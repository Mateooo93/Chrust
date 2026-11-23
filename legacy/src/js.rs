#![allow(dead_code)]
//! JavaScript runtime built on Boa, with a hand-written DOM binding layer.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use boa_engine::{
    js_string, property::Attribute, Context, JsNativeError, JsObject, JsResult, JsString, JsValue,
    NativeFunction, Source,
};
use boa_engine::object::builtins::{JsArray, JsFunction, JsPromise};
use boa_engine::property::{PropertyDescriptor, PropertyKey};

use crate::css::{parse_declarations, parse_selector, Declaration, MatchContext};
use crate::dom::{Document, NodeData, NodeId};
use crate::page::ScriptItem;

fn key_string<K: Into<PropertyKey>>(k: K) -> String {
    match k.into() {
        PropertyKey::String(s) => s.to_std_string_escaped(),
        PropertyKey::Index(i) => i.get().to_string(),
        PropertyKey::Symbol(_) => String::new(),
    }
}

enum PendingProp {
    Value(String, JsValue),
    Accessor(String, Option<JsFunction>, Option<JsFunction>),
    Method(String, NativeFunction),
}

/// Collects definitions so functions can be created without holding a mutable
/// borrow of the context (unlike ObjectInitializer).
struct ObjBuilder {
    items: Vec<PendingProp>,
}

impl ObjBuilder {
    fn new() -> ObjBuilder {
        ObjBuilder { items: Vec::new() }
    }
    fn property<K: Into<PropertyKey>, V: Into<JsValue>>(&mut self, key: K, value: V, _attr: Attribute) {
        self.items.push(PendingProp::Value(key_string(key), value.into()));
    }
    fn accessor<K: Into<PropertyKey>>(
        &mut self,
        key: K,
        get: Option<JsFunction>,
        set: Option<JsFunction>,
        _attr: Attribute,
    ) {
        self.items.push(PendingProp::Accessor(key_string(key), get, set));
    }
    fn function<K: Into<PropertyKey>>(&mut self, function: NativeFunction, key: K, _length: usize) {
        self.items.push(PendingProp::Method(key_string(key), function));
    }
    fn build(&mut self, ctx: &mut Context) -> JsObject {
        let obj = JsObject::with_object_proto(ctx.intrinsics());
        for item in self.items.drain(..) {
            match item {
                PendingProp::Value(key, value) => {
                    let d = PropertyDescriptor::builder()
                        .value(value)
                        .writable(true)
                        .enumerable(true)
                        .configurable(true);
                    let _ = obj.define_property_or_throw(JsString::from(key.as_str()), d, ctx);
                }
                PendingProp::Accessor(key, get, set) => {
                    let mut d = PropertyDescriptor::builder()
                        .enumerable(true)
                        .configurable(true);
                    if let Some(g) = get {
                        d = d.get(g);
                    }
                    if let Some(s) = set {
                        d = d.set(s);
                    }
                    let _ = obj.define_property_or_throw(JsString::from(key.as_str()), d, ctx);
                }
                PendingProp::Method(key, nf) => {
                    let f = nf.to_js_function(ctx.realm());
                    let d = PropertyDescriptor::builder()
                        .value(f)
                        .writable(true)
                        .enumerable(false)
                        .configurable(true);
                    let _ = obj.define_property_or_throw(JsString::from(key.as_str()), d, ctx);
                }
            }
        }
        obj
    }
}


#[derive(Clone)]
pub struct Shared {
    pub dom: Rc<RefCell<Document>>,
    pub listeners: Rc<RefCell<Vec<ListenerEntry>>>,
    pub timers: Rc<RefCell<Vec<usize>>>,
    pub storage: Rc<RefCell<HashMap<String, String>>>,
    pub url: Rc<RefCell<String>>,
    pub pending_nav: Rc<RefCell<Option<String>>>,
    pub changed: Rc<Cell<bool>>,
    pub prevented: Rc<Cell<bool>>,
    pub stopped: Rc<Cell<bool>>,
    pub inner_size: (f64, f64),
}

#[derive(Clone)]
struct ListenerEntry {
    node: Option<NodeId>,
    event: String,
    func_index: usize,
}

pub struct JsRuntime {
    pub ctx: Context,
    pub shared: Shared,
    pub errors: Vec<String>,
}

fn jsstr(s: &str) -> JsString {
    JsString::from(s)
}

fn val_str(v: &JsValue, ctx: &mut Context) -> String {
    v.to_string(ctx)
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default()
}

fn arg_str(args: &[JsValue], i: usize, ctx: &mut Context) -> String {
    args.get(i).map(|v| val_str(v, ctx)).unwrap_or_default()
}

fn node_of(shared: &Shared, v: &JsValue, ctx: &mut Context) -> Option<NodeId> {
    let obj = v.as_object()?;
    let n = obj.get(js_string!("__auroraNode"), ctx).ok()?;
    n.as_number().map(|n| n as usize)
}

fn mk_get<F>(ctx: &mut Context, f: F) -> JsFunction
where
    F: Fn(&mut Context) -> JsResult<JsValue> + 'static,
{
    unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| f(ctx))
            .to_js_function(ctx.realm())
    }
}

fn mk_set<F>(ctx: &mut Context, f: F) -> JsFunction
where
    F: Fn(JsValue, &mut Context) -> JsResult<()> + 'static,
{
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let v = args.first().cloned().unwrap_or_default();
            f(v, ctx)?;
            Ok(JsValue::undefined())
        })
        .to_js_function(ctx.realm())
    }
}

fn mk_method<F>(f: F) -> NativeFunction
where
    F: Fn(&[JsValue], &mut Context) -> JsResult<JsValue> + 'static,
{
    unsafe { NativeFunction::from_closure(move |_this, args, ctx| f(args, ctx)) }
}

fn mk_method_this<F>(f: F) -> NativeFunction
where
    F: Fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue> + 'static,
{
    unsafe { NativeFunction::from_closure(move |this, args, ctx| f(this, args, ctx)) }
}

fn style_of(node: NodeId, doc: &Document) -> Vec<Declaration> {
    doc.attr(node, "style")
        .map(|s| parse_declarations(s))
        .unwrap_or_default()
}

fn inline_style_get(doc: &Document, node: NodeId, prop: &str) -> String {
    style_of(node, doc)
        .into_iter()
        .find(|d| d.name == prop)
        .map(|d| d.value)
        .unwrap_or_default()
}

fn inline_style_set(doc: &mut Document, node: NodeId, prop: &str, value: &str) {
    let mut decls = style_of(node, doc);
    if value.trim().is_empty() {
        decls.retain(|d| d.name != prop);
    } else if let Some(d) = decls.iter_mut().find(|d| d.name == prop) {
        d.value = value.to_string();
    } else {
        decls.push(Declaration {
            name: prop.to_string(),
            value: value.to_string(),
            important: false,
        });
    }
    let serialized = decls
        .iter()
        .map(|d| format!("{}: {};", d.name, d.value))
        .collect::<Vec<_>>()
        .join(" ");
    doc.set_attr(node, "style", &serialized);
    doc.remove_attr(node, "x-aurora-noop");
}

fn class_list_for(shared: &Shared, ctx: &mut Context, node: NodeId) -> JsValue {
    let base = shared.clone();
    let mut init = ObjBuilder::new();
    {
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let class = arg_str(args, 0, ctx);
                let mut doc = s.dom.borrow_mut();
                let mut classes = doc
                    .attr(node, "class")
                    .map(|c| c.split_whitespace().map(|x| x.to_string()).collect::<Vec<_>>())
                    .unwrap_or_default();
                if !classes.iter().any(|c| c == &class) {
                    classes.push(class);
                    doc.set_attr(node, "class", &classes.join(" "));
                }
                s.changed.set(true);
                Ok(JsValue::undefined())
            }),
            js_string!("add"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let class = arg_str(args, 0, ctx);
                let mut doc = s.dom.borrow_mut();
                let mut classes = doc
                    .attr(node, "class")
                    .map(|c| c.split_whitespace().map(|x| x.to_string()).collect::<Vec<_>>())
                    .unwrap_or_default();
                let before = classes.len();
                classes.retain(|c| c != &class);
                if classes.len() != before {
                    doc.set_attr(node, "class", &classes.join(" "));
                    s.changed.set(true);
                }
                Ok(JsValue::undefined())
            }),
            js_string!("remove"),
            1,
        );
        let s2 = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let class = arg_str(args, 0, ctx);
                let doc = s2.dom.borrow();
                let has = doc
                    .attr(node, "class")
                    .map(|c| c.split_whitespace().any(|x| x == class))
                    .unwrap_or(false);
                Ok(JsValue::from(has))
            }),
            js_string!("contains"),
            1,
        );
        let s2 = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let class = arg_str(args, 0, ctx);
                let mut doc = s2.dom.borrow_mut();
                let mut classes = doc
                    .attr(node, "class")
                    .map(|c| c.split_whitespace().map(|x| x.to_string()).collect::<Vec<_>>())
                    .unwrap_or_default();
                let had = classes.iter().any(|c| c == &class);
                if had {
                    classes.retain(|c| c != &class);
                } else {
                    classes.push(class);
                }
                doc.set_attr(node, "class", &classes.join(" "));
                drop(doc);
                s2.changed.set(true);
                Ok(JsValue::from(!had))
            }),
            js_string!("toggle"),
            1,
        );
    }
    JsValue::from(init.build(ctx))
}

fn style_object_for(shared: &Shared, ctx: &mut Context, node: NodeId) -> JsValue {
    let props: &[(&str, &str)] = &[
        ("display", "display"),
        ("color", "color"),
        ("background", "background"),
        ("backgroundColor", "background-color"),
        ("backgroundImage", "background-image"),
        ("width", "width"),
        ("height", "height"),
        ("minWidth", "min-width"),
        ("maxWidth", "max-width"),
        ("margin", "margin"),
        ("marginTop", "margin-top"),
        ("marginRight", "margin-right"),
        ("marginBottom", "margin-bottom"),
        ("marginLeft", "margin-left"),
        ("padding", "padding"),
        ("paddingTop", "padding-top"),
        ("paddingRight", "padding-right"),
        ("paddingBottom", "padding-bottom"),
        ("paddingLeft", "padding-left"),
        ("fontSize", "font-size"),
        ("fontWeight", "font-weight"),
        ("fontFamily", "font-family"),
        ("fontStyle", "font-style"),
        ("lineHeight", "line-height"),
        ("textAlign", "text-align"),
        ("textDecoration", "text-decoration"),
        ("visibility", "visibility"),
        ("opacity", "opacity"),
        ("position", "position"),
        ("top", "top"),
        ("right", "right"),
        ("bottom", "bottom"),
        ("left", "left"),
        ("border", "border"),
        ("borderRadius", "border-radius"),
        ("overflow", "overflow"),
        ("flex", "flex"),
        ("flexDirection", "flex-direction"),
        ("justifyContent", "justify-content"),
        ("alignItems", "align-items"),
        ("gap", "gap"),
        ("transform", "transform"),
        ("zIndex", "z-index"),
        ("cursor", "cursor"),
        ("whiteSpace", "white-space"),
    ];
    let mut init = ObjBuilder::new();
    for (js_name, css_name) in props {
        let s = shared.clone();
        let css = css_name.to_string();
        let get = mk_get(ctx, move |_ctx| {
            let doc = s.dom.borrow();
            Ok(JsValue::from(jsstr(&inline_style_get(&doc, node, &css))))
        });
        let s = shared.clone();
        let css = css_name.to_string();
        let set = mk_set(ctx, move |v, ctx| {
            let text = val_str(&v, ctx);
            let mut doc = s.dom.borrow_mut();
            inline_style_set(&mut doc, node, &css, &text);
            drop(doc);
            s.changed.set(true);
            Ok(())
        });
        init.accessor(jsstr(js_name), Some(get), Some(set), Attribute::all());
    }
    {
        let s = shared.clone();
        init.function(
            mk_method(move |args, ctx| {
                let prop = arg_str(args, 0, ctx);
                let doc = s.dom.borrow();
                let css = camel_to_css(&prop);
                Ok(JsValue::from(jsstr(&inline_style_get(&doc, node, &css))))
            }),
            js_string!("getPropertyValue"),
            1,
        );
        let s = shared.clone();
        init.function(
            mk_method(move |args, ctx| {
                let prop = arg_str(args, 0, ctx);
                let value = arg_str(args, 1, ctx);
                let mut doc = s.dom.borrow_mut();
                inline_style_set(&mut doc, node, &camel_to_css(&prop), &value);
                drop(doc);
                s.changed.set(true);
                Ok(JsValue::undefined())
            }),
            js_string!("setProperty"),
            2,
        );
        init.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("removeProperty"),
            1,
        );
    }
    JsValue::from(init.build(ctx))
}

fn camel_to_css(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_uppercase() {
            out.push('-');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn query_all(shared: &Shared, root: NodeId, selector: &str, include_root: bool) -> Vec<NodeId> {
    let doc = shared.dom.borrow();
    let mut sels = Vec::new();
    for part in crate::css::split_top_level(selector, ',') {
        if let Some(sel) = parse_selector(&part) {
            sels.push(sel);
        }
    }
    if sels.is_empty() {
        return Vec::new();
    }
    let ctx = MatchContext { doc: &doc, hovered: None, active: None, focused: None };
    let mut out = Vec::new();
    let walk = doc.walk(root);
    for (i, n) in walk.into_iter().enumerate() {
        if i == 0 && !include_root {
            continue;
        }
        if !doc.is_element(n) {
            continue;
        }
        if sels.iter().any(|s| s.matches(n, &ctx)) {
            out.push(n);
        }
    }
    out
}

fn wrap_nodes(shared: &Shared, ctx: &mut Context, nodes: &[NodeId]) -> JsValue {
    let arr = JsArray::new(ctx);
    for &n in nodes {
        let _ = arr.push(wrap(shared, ctx, n), ctx);
    }
    JsValue::from(arr)
}

fn set_attr_flag(shared: &Shared, node: NodeId, name: &str, value: bool) {
    let mut doc = shared.dom.borrow_mut();
    if value {
        doc.set_attr(node, name, "");
    } else {
        doc.remove_attr(node, name);
    }
    drop(doc);
    shared.changed.set(true);
}

fn attr_flag(shared: &Shared, node: NodeId, name: &str) -> bool {
    shared.dom.borrow().attr(node, name).is_some()
}

fn parent_of(shared: &Shared, node: NodeId) -> Option<NodeId> {
    shared.dom.borrow().nodes[node].parent
}

fn push_rooted(ctx: &mut Context, func: JsFunction) -> JsResult<usize> {
    let global = ctx.global_object();
    let arr_val = global.get(js_string!("__auroraFns"), ctx)?;
    let arr = match arr_val.as_object().cloned().and_then(|o| JsArray::from_object(o).ok()) {
        Some(a) => a,
        None => {
            return Err(JsNativeError::typ()
                .with_message("listener storage missing")
                .into())
        }
    };
    arr.push(func, ctx)?;
    let len = arr.length(ctx)?;
    Ok(len as usize - 1)
}

fn get_rooted(ctx: &mut Context, index: usize) -> Option<JsFunction> {
    let global = ctx.global_object();
    let arr_val = global.get(js_string!("__auroraFns"), ctx).ok()?;
    let arr = arr_val.as_object().cloned().and_then(|o| JsArray::from_object(o).ok())?;
    let v = arr.get(index as u64, ctx).ok()?;
    v.as_object().cloned().and_then(JsFunction::from_object)
}

const BOOTSTRAP_JS: &str = r#"
(function () {
    ["onload", "onerror", "onresize", "onscroll", "onclick", "onkeydown", "onkeyup",
     "onmousemove", "onmessage", "onbeforeunload", "onpopstate", "onhashchange",
     "onfocus", "onblur"].forEach(function (k) {
        if (!(k in window)) { try { window[k] = null; } catch (e) {} }
    });
    if (typeof window.Node === "undefined") {
        window.Node = function Node() {};
        window.Element = function Element() {};
        window.HTMLElement = function HTMLElement() {};
        window.HTMLDocument = function HTMLDocument() {};
        window.Document = function Document() {};
        window.Window = function Window() {};
    }
    if (typeof window.CustomEvent === "undefined") {
        window.CustomEvent = function CustomEvent(type, init) {
            this.type = type;
            this.detail = init && init.detail;
            this.defaultPrevented = false;
        };
        window.CustomEvent.prototype.preventDefault = function () { this.defaultPrevented = true; };
        window.CustomEvent.prototype.stopPropagation = function () {};
    }
    if (typeof window.URLSearchParams === "undefined") {
        function decode(s) { try { return decodeURIComponent(String(s).replace(/\+/g, " ")); } catch (e) { return String(s); } }
        function encode(s) { return encodeURIComponent(String(s)); }
        var URLSearchParams = function URLSearchParams(init) {
            var pairs = [];
            if (typeof init === "string") {
                init.replace(/^\?/, "").split("&").forEach(function (part) {
                    if (!part) return;
                    var i = part.indexOf("=");
                    if (i < 0) pairs.push([decode(part), ""]);
                    else pairs.push([decode(part.slice(0, i)), decode(part.slice(i + 1))]);
                });
            } else if (init && typeof init === "object") {
                Object.keys(init).forEach(function (k) {
                    var v = init[k];
                    if (Array.isArray(v)) v.forEach(function (x) { pairs.push([String(k), String(x)]); });
                    else pairs.push([String(k), String(v)]);
                });
            }
            this._pairs = pairs;
        };
        URLSearchParams.prototype.get = function (name) {
            for (var i = 0; i < this._pairs.length; i++) if (this._pairs[i][0] === name) return this._pairs[i][1];
            return null;
        };
        URLSearchParams.prototype.getAll = function (name) {
            return this._pairs.filter(function (p) { return p[0] === name; }).map(function (p) { return p[1]; });
        };
        URLSearchParams.prototype.has = function (name) {
            return this._pairs.some(function (p) { return p[0] === name; });
        };
        URLSearchParams.prototype.append = function (name, value) { this._pairs.push([String(name), String(value)]); };
        URLSearchParams.prototype.set = function (name, value) {
            var found = false; var out = [];
            for (var i = 0; i < this._pairs.length; i++) {
                if (this._pairs[i][0] === name) { if (!found) { out.push([name, String(value)]); found = true; } }
                else out.push(this._pairs[i]);
            }
            if (!found) out.push([name, String(value)]);
            this._pairs = out;
        };
        URLSearchParams.prototype["delete"] = function (name) {
            this._pairs = this._pairs.filter(function (p) { return p[0] !== name; });
        };
        URLSearchParams.prototype.toString = function () {
            return this._pairs.map(function (p) { return encode(p[0]) + "=" + encode(p[1]); }).join("&");
        };
        URLSearchParams.prototype.forEach = function (fn, thisArg) {
            this._pairs.forEach(function (p) { fn.call(thisArg, p[1], p[0], this); }, this);
        };
        URLSearchParams.prototype.entries = function () { return this._pairs.map(function (p) { return [p[0], p[1]]; })[Symbol.iterator](); };
        URLSearchParams.prototype.keys = function () { return this._pairs.map(function (p) { return p[0]; })[Symbol.iterator](); };
        URLSearchParams.prototype.values = function () { return this._pairs.map(function (p) { return p[1]; })[Symbol.iterator](); };
        window.URLSearchParams = URLSearchParams;
    }
    if (typeof window.URL === "undefined") {
        var URL = function URL(href, base) {
            var s = String(href);
            if (base && !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(s)) {
                try {
                    var b = String(base);
                    var m = b.match(/^([a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^\/]*)/);
                    if (m) s = m[1] + (s.charAt(0) === "/" ? "" : "/") + s;
                } catch (e) {}
            }
            var m = s.match(/^([a-zA-Z][a-zA-Z0-9+.-]*:)(\/\/([^\/?#]*))?([^?#]*)(\?[^#]*)?(#.*)?$/);
            this.href = s;
            this.protocol = m ? m[1] : "";
            var auth = m && m[3] ? m[3] : "";
            this.host = auth;
            var hm = auth.match(/^(.*?)(?::(\d+))?$/);
            this.hostname = hm ? hm[1] : "";
            this.port = hm && hm[2] ? hm[2] : "";
            this.origin = this.protocol + "//" + this.host;
            this.pathname = (m && m[4] ? m[4] : "") || "/";
            this.search = m && m[5] ? m[5] : "";
            this.hash = m && m[6] ? m[6] : "";
        };
        URL.prototype.toString = function () { return this.href; };
        Object.defineProperty(URL.prototype, "searchParams", {
            get: function () { return new URLSearchParams(this.search); }
        });
        URL.createObjectURL = function () { return "blob:aurora/0"; };
        URL.revokeObjectURL = function () {};
        window.URL = URL;
    }
    if (typeof window.btoa === "undefined") {
        var B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        window.btoa = function (input) {
            var str = String(input); var out = "";
            for (var i = 0; i < str.length; i += 3) {
                var a = str.charCodeAt(i), b = str.charCodeAt(i + 1), c = str.charCodeAt(i + 2);
                var n = (a << 16) | ((isNaN(b) ? 0 : b) << 8) | (isNaN(c) ? 0 : c);
                out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (isNaN(b) ? "=" : B64[(n >> 6) & 63]) + (isNaN(c) ? "=" : B64[n & 63]);
            }
            return out;
        };
        window.atob = function (input) {
            var str = String(input).replace(/[^A-Za-z0-9+/=]/g, ""); var out = "";
            for (var i = 0; i < str.length; i += 4) {
                var n = (B64.indexOf(str[i]) << 18) | (B64.indexOf(str[i + 1]) << 12) | ((str[i + 2] === "=" ? 0 : B64.indexOf(str[i + 2])) << 6) | (str[i + 3] === "=" ? 0 : B64.indexOf(str[i + 3]));
                out += String.fromCharCode((n >> 16) & 255);
                if (str[i + 2] !== "=") out += String.fromCharCode((n >> 8) & 255);
                if (str[i + 3] !== "=") out += String.fromCharCode(n & 255);
            }
            return out;
        };
    }
    if (typeof window.TextEncoder === "undefined") {
        window.TextEncoder = function TextEncoder() {};
        window.TextEncoder.prototype.encode = function (s) {
            var str = unescape(encodeURIComponent(String(s)));
            var arr = new Uint8Array(str.length);
            for (var i = 0; i < str.length; i++) arr[i] = str.charCodeAt(i);
            return arr;
        };
        window.TextDecoder = function TextDecoder() {};
        window.TextDecoder.prototype.decode = function (buf) {
            if (!buf) return "";
            var bytes = new Uint8Array(buf.buffer || buf, buf.byteOffset || 0, buf.byteLength || buf.length || 0);
            var s = "";
            for (var i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
            try { return decodeURIComponent(escape(s)); } catch (e) { return s; }
        };
    }
})();
"#;

fn dispatch_event(shared: &Shared, ctx: &mut Context, target: Option<NodeId>, event_type: &str) -> JsResult<bool> {
    shared.prevented.set(false);
    shared.stopped.set(false);
    shared.changed.set(false);
    let doc = shared.dom.borrow();
    let mut path: Vec<Option<NodeId>> = Vec::new();
    let mut cur = target;
    while let Some(n) = cur {
        if doc.is_element(n) || n == doc.root {
            path.push(Some(n));
        }
        cur = doc.nodes[n].parent;
    }
    drop(doc);
    // Make sure the document and window are in the path.
    let root = shared.dom.borrow().root;
    if !path.iter().any(|p| *p == Some(root)) {
        path.push(Some(root));
    }
    path.push(None);

    // Window-level `on<event>` properties and <body onload> style handlers.
    if target.is_none() {
        if event_type == "load" || event_type == "DOMContentLoaded" {
            let body = {
                let doc = shared.dom.borrow();
                doc.walk(doc.root)
                    .into_iter()
                    .find(|&n| doc.tag(n) == Some("body"))
            };
            if let Some(body) = body {
                path.insert(0, Some(body));
            }
        }
    }
    let event = make_event(shared, ctx, event_type, target);
    let mut prevented = false;
    'outer: for t in path {
        if shared.stopped.get() {
            break;
        }
        // Update currentTarget.
        if let Some(obj) = event.as_object() {
            let cur_val = match t {
                Some(n) => wrap(shared, ctx, n),
                None => JsValue::undefined(),
            };
            let _ = obj.set(js_string!("currentTarget"), cur_val, false, ctx);
        }
        let listeners: Vec<usize> = {
            let l = shared.listeners.borrow();
            l.iter()
                .filter(|e| e.node == t && e.event == event_type)
                .map(|e| e.func_index)
                .collect()
        };
        for idx in listeners {
            if let Some(func) = get_rooted(ctx, idx) {
                let this = match t {
                    Some(n) => wrap(shared, ctx, n),
                    None => ctx.global_object().into(),
                };
                let _ = func.call(&this, &[event.clone()], ctx);
                if shared.prevented.get() {
                    prevented = true;
                }
            }
        }
        // Window on<event> property handler.
        if t.is_none() {
            let handler = ctx
                .global_object()
                .get(js_string!(format!("on{}", event_type)), ctx)
                .ok();
            if let Some(handler) = handler {
                if let Some(func) = handler.as_object().cloned().and_then(JsFunction::from_object) {
                    let this = ctx.global_object().into();
                    let _ = func.call(&this, &[event.clone()], ctx);
                    if shared.prevented.get() {
                        prevented = true;
                    }
                }
            }
        }
        // Inline handlers.
        if let Some(n) = t {
            let handler = {
                let doc = shared.dom.borrow();
                doc.attr(n, &format!("on{}", event_type))
                    .map(|s| s.to_string())
            };
            if let Some(code) = handler {
                let src = format!("(function(event) {{ {} }})", code);
                if let Ok(v) = ctx.eval(Source::from_bytes(&src)) {
                    if let Some(func) = v.as_object().cloned().and_then(JsFunction::from_object) {
                        let this = wrap(shared, ctx, n);
                        let _ = func.call(&this, &[event.clone()], ctx);
                    }
                }
            }
        }
        if shared.stopped.get() {
            break 'outer;
        }
    }
    if shared.prevented.get() {
        prevented = true;
    }
    Ok(prevented)
}

fn make_event(shared: &Shared, ctx: &mut Context, event_type: &str, target: Option<NodeId>) -> JsValue {
    let mut init = ObjBuilder::new();
    let target_val = match target {
        Some(n) => wrap(shared, ctx, n),
        None => JsValue::undefined(),
    };
    init.property(js_string!("type"), jsstr(event_type), Attribute::all());
    init.property(js_string!("target"), target_val.clone(), Attribute::all());
    init.property(js_string!("currentTarget"), target_val, Attribute::all());
    init.property(js_string!("bubbles"), true, Attribute::all());
    init.property(js_string!("cancelable"), true, Attribute::all());
    let s = shared.clone();
    init.function(
        mk_method(move |_args, _ctx| {
            s.prevented.set(true);
            Ok(JsValue::undefined())
        }),
        js_string!("preventDefault"),
        0,
    );
    let s = shared.clone();
    init.function(
        mk_method(move |_args, _ctx| {
            s.stopped.set(true);
            Ok(JsValue::undefined())
        }),
        js_string!("stopPropagation"),
        0,
    );
    let s = shared.clone();
    init.function(
        mk_method(move |_args, _ctx| {
            s.stopped.set(true);
            Ok(JsValue::undefined())
        }),
        js_string!("stopImmediatePropagation"),
        0,
    );
    let s = shared.clone();
    let get = mk_get(ctx, move |_ctx| Ok(JsValue::from(s.prevented.get())));
    init.accessor(js_string!("defaultPrevented"), Some(get), None, Attribute::all());
    JsValue::from(init.build(ctx))
}

fn make_response(
    shared: &Shared,
    ctx: &mut Context,
    status: u16,
    status_text: &str,
    url: &str,
    body: &str,
    content_type: &str,
) -> JsValue {
    let mut init = ObjBuilder::new();
    init.property(js_string!("ok"), (200..300).contains(&status), Attribute::all());
    init.property(js_string!("status"), status as f64, Attribute::all());
    init.property(js_string!("statusText"), jsstr(status_text), Attribute::all());
    init.property(js_string!("url"), jsstr(url), Attribute::all());
    init.property(js_string!("redirected"), false, Attribute::all());
    init.property(js_string!("type"), js_string!("basic"), Attribute::all());

    let mut headers = ObjBuilder::new();
    let ct = content_type.to_string();
    {
        let ct = ct.clone();
        headers.function(
            mk_method(move |args, ctx| {
                let name = arg_str(args, 0, ctx).to_ascii_lowercase();
                let value = if name == "content-type" {
                    ct.clone()
                } else {
                    String::new()
                };
                Ok(JsValue::from(jsstr(&value)))
            }),
            js_string!("get"),
            1,
        );
        headers.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("forEach"),
            1,
        );
    }
    init.property(js_string!("headers"), JsValue::from(headers.build(ctx)), Attribute::all());

    let body_text = body.to_string();
    init.function(
        mk_method(move |_args, ctx| {
            Ok(JsValue::from(JsPromise::resolve(jsstr(&body_text), ctx)))
        }),
        js_string!("text"),
        0,
    );
    let body_json = body.to_string();
    init.function(
        mk_method(move |_args, ctx| {
            let global = ctx.global_object();
            let json = global.get(js_string!("JSON"), ctx)?;
            let parse = if let Some(obj) = json.as_object() {
                obj.get(js_string!("parse"), ctx)?
            } else {
                JsValue::undefined()
            };
            let parsed = if let Some(obj) = parse.as_object() {
                obj.call(&json, &[JsValue::from(jsstr(&body_json))], ctx)
                    .unwrap_or(JsValue::null())
            } else {
                JsValue::null()
            };
            Ok(JsValue::from(JsPromise::resolve(parsed, ctx)))
        }),
        js_string!("json"),
        0,
    );
    let body_ab = body.as_bytes().to_vec();
    init.function(
        mk_method(move |_args, _ctx| {
            let _ = &body_ab;
            Ok(JsValue::null())
        }),
        js_string!("arrayBuffer"),
        0,
    );
    init.function(
        mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
        js_string!("clone"),
        0,
    );
    let _ = shared;
    JsValue::from(init.build(ctx))
}

// ----------------------------------------------------------------- wrappers

pub fn wrap(shared: &Shared, ctx: &mut Context, node: NodeId) -> JsValue {
    let is_element = {
        let doc = shared.dom.borrow();
        doc.is_element(node)
    };
    if !is_element {
        let mut init = ObjBuilder::new();
        init.property(js_string!("__auroraNode"), node as f64, Attribute::all());
        init.property(js_string!("nodeType"), 3.0, Attribute::all());
        let s = shared.clone();
        let get = mk_get(ctx, move |_ctx| {
            let doc = s.dom.borrow();
            Ok(JsValue::from(jsstr(doc.text(node).unwrap_or(""))))
        });
        let s = shared.clone();
        let set = mk_set(ctx, move |v, ctx| {
            let text = val_str(&v, ctx);
            let mut doc = s.dom.borrow_mut();
            doc.set_text_content(node, &text);
            drop(doc);
            s.changed.set(true);
            Ok(())
        });
        init.accessor(js_string!("textContent"), Some(get), Some(set), Attribute::all());
        let s = shared.clone();
        let get = mk_get(ctx, move |_ctx| {
            let doc = s.dom.borrow();
            Ok(match doc.nodes[node].parent {
                Some(p) => wrap(&s, _ctx, p),
                None => JsValue::null(),
            })
        });
        init.accessor(js_string!("parentNode"), Some(get), None, Attribute::all());
        let s = shared.clone();
        let get = mk_get(ctx, move |_ctx| Ok(JsValue::from(jsstr(""))));
        let _ = (s, get);
        init.accessor(
            js_string!("nodeName"),
            Some(mk_get(ctx, |_| Ok(JsValue::from(jsstr("#text"))))),
            None,
            Attribute::all(),
        );
        return JsValue::from(init.build(ctx));
    }

    let s = shared.clone();
    let base = s.clone();
    let mut init = ObjBuilder::new();
    init.property(js_string!("__auroraNode"), node as f64, Attribute::all());

    // ---- basic properties
    {
        let s = base.clone();
        init.accessor(
            js_string!("nodeType"),
            Some(mk_get(ctx, move |ctx| {
                let _ = &s;
                let doc = s.dom.borrow();
                let t = match doc.nodes[node].data {
                    NodeData::Element(_) => 1,
                    NodeData::Text(_) => 3,
                    NodeData::Comment(_) => 8,
                    _ => 9,
                };
                let _ = ctx;
                Ok(JsValue::from(t))
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("tagName"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                let tag = doc.tag(node).unwrap_or("").to_ascii_uppercase();
                Ok(JsValue::from(jsstr(&tag)))
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("nodeName"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                let tag = doc.tag(node).unwrap_or("").to_ascii_uppercase();
                Ok(JsValue::from(jsstr(&tag)))
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        let s_set = base.clone();
        init.accessor(
            js_string!("id"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                Ok(JsValue::from(jsstr(doc.attr(node, "id").unwrap_or(""))))
            })),
            Some(mk_set(ctx, move |v, ctx| {
                let text = val_str(&v, ctx);
                let mut doc = s_set.dom.borrow_mut();
                doc.set_attr(node, "id", &text);
                drop(doc);
                s_set.changed.set(true);
                Ok(())
            })),
            Attribute::all(),
        );
        let s = base.clone();
        let s_set = base.clone();
        init.accessor(
            js_string!("className"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                Ok(JsValue::from(jsstr(doc.attr(node, "class").unwrap_or(""))))
            })),
            Some(mk_set(ctx, move |v, ctx| {
                let text = val_str(&v, ctx);
                let mut doc = s_set.dom.borrow_mut();
                doc.set_attr(node, "class", &text);
                drop(doc);
                s_set.changed.set(true);
                Ok(())
            })),
            Attribute::all(),
        );
        let s = base.clone();
        let s_set = base.clone();
        init.accessor(
            js_string!("textContent"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                Ok(JsValue::from(jsstr(&doc.text_content(node))))
            })),
            Some(mk_set(ctx, move |v, ctx| {
                let text = val_str(&v, ctx);
                let mut doc = s_set.dom.borrow_mut();
                doc.set_text_content(node, &text);
                drop(doc);
                s_set.changed.set(true);
                Ok(())
            })),
            Attribute::all(),
        );
        let s = base.clone();
        let s_set = base.clone();
        init.accessor(
            js_string!("innerHTML"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                Ok(JsValue::from(jsstr(&doc.inner_html(node))))
            })),
            Some(mk_set(ctx, move |v, ctx| {
                let text = val_str(&v, ctx);
                let mut doc = s_set.dom.borrow_mut();
                crate::html::set_inner_html(&mut doc, node, &text);
                drop(doc);
                s_set.changed.set(true);
                Ok(())
            })),
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("outerHTML"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                Ok(JsValue::from(jsstr(&doc.outer_html(node))))
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        let s_set = base.clone();
        init.accessor(
            js_string!("innerText"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                Ok(JsValue::from(jsstr(&doc.text_content(node))))
            })),
            Some(mk_set(ctx, move |v, ctx| {
                let text = val_str(&v, ctx);
                let mut doc = s_set.dom.borrow_mut();
                doc.set_text_content(node, &text);
                drop(doc);
                s_set.changed.set(true);
                Ok(())
            })),
            Attribute::all(),
        );
    }

    // Simple reflected attributes.
    for (js_name, attr_name) in [
        ("value", "value"),
        ("src", "src"),
        ("href", "href"),
        ("alt", "alt"),
        ("title", "title"),
        ("name", "name"),
        ("placeholder", "placeholder"),
        ("type", "type"),
        ("htmlFor", "for"),
    ] {
        let s = base.clone();
        let attr = attr_name.to_string();
        let get = mk_get(ctx, move |_ctx| {
            let doc = s.dom.borrow();
            let v = if attr == "value" && doc.tag(node) == Some("textarea") {
                doc.text_content(node)
            } else {
                doc.attr(node, &attr).unwrap_or("").to_string()
            };
            Ok(JsValue::from(jsstr(&v)))
        });
        let s = base.clone();
        let attr = attr_name.to_string();
        let set = mk_set(ctx, move |v, ctx| {
            let text = val_str(&v, ctx);
            let mut doc = s.dom.borrow_mut();
            if attr == "value" && doc.tag(node) == Some("textarea") {
                doc.set_text_content(node, &text);
            } else {
                doc.set_attr(node, &attr, &text);
            }
            drop(doc);
            s.changed.set(true);
            Ok(())
        });
        init.accessor(jsstr(js_name), Some(get), Some(set), Attribute::all());
    }
    for flag in ["checked", "selected", "disabled", "readOnly", "hidden", "required"] {
        let s = base.clone();
        let attr = match flag {
            "readOnly" => "readonly",
            other => other,
        }
        .to_string();
        let attr_set = attr.clone();
        let get = mk_get(ctx, move |_ctx| {
            let _ = &s;
            let doc = s.dom.borrow();
            Ok(JsValue::from(doc.attr(node, &attr).is_some()))
        });
        let s = base.clone();
        let set = mk_set(ctx, move |v, ctx| {
            let on = v.to_boolean();
            let mut doc = s.dom.borrow_mut();
            if on {
                doc.set_attr(node, &attr_set, "");
            } else {
                doc.remove_attr(node, &attr_set);
            }
            drop(doc);
            s.changed.set(true);
            let _ = ctx;
            Ok(())
        });
        init.accessor(jsstr(flag), Some(get), Some(set), Attribute::all());
    }

    // Navigation relations.
    {
        let s = base.clone();
        init.accessor(
            js_string!("parentNode"),
            Some(mk_get(ctx, move |_ctx| {
                Ok(match parent_of(&s, node) {
                    Some(p) => wrap(&s, _ctx, p),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("parentElement"),
            Some(mk_get(ctx, move |_ctx| {
                let p = parent_of(&s, node).filter(|p| s.dom.borrow().is_element(*p));
                Ok(match p {
                    Some(p) => wrap(&s, _ctx, p),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("children"),
            Some(mk_get(ctx, move |_ctx| {
                let kids = {
                    let doc = s.dom.borrow();
                    doc.element_children(node)
                };
                Ok(wrap_nodes(&s, _ctx, &kids))
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("childNodes"),
            Some(mk_get(ctx, move |_ctx| {
                let kids = {
                    let doc = s.dom.borrow();
                    doc.nodes[node].children.clone()
                };
                Ok(wrap_nodes(&s, _ctx, &kids))
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("firstChild"),
            Some(mk_get(ctx, move |_ctx| {
                let c = {
                    let doc = s.dom.borrow();
                    doc.nodes[node].children.first().copied()
                };
                Ok(match c {
                    Some(c) => wrap(&s, _ctx, c),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("lastChild"),
            Some(mk_get(ctx, move |_ctx| {
                let c = {
                    let doc = s.dom.borrow();
                    doc.nodes[node].children.last().copied()
                };
                Ok(match c {
                    Some(c) => wrap(&s, _ctx, c),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("firstElementChild"),
            Some(mk_get(ctx, move |_ctx| {
                let c = {
                    let doc = s.dom.borrow();
                    doc.element_children(node).first().copied()
                };
                Ok(match c {
                    Some(c) => wrap(&s, _ctx, c),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("lastElementChild"),
            Some(mk_get(ctx, move |_ctx| {
                let c = {
                    let doc = s.dom.borrow();
                    doc.element_children(node).last().copied()
                };
                Ok(match c {
                    Some(c) => wrap(&s, _ctx, c),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("nextSibling"),
            Some(mk_get(ctx, move |_ctx| {
                let c = sibling_of(&s, node, 1);
                Ok(match c {
                    Some(c) => wrap(&s, _ctx, c),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("previousSibling"),
            Some(mk_get(ctx, move |_ctx| {
                let c = sibling_of(&s, node, -1);
                Ok(match c {
                    Some(c) => wrap(&s, _ctx, c),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("nextElementSibling"),
            Some(mk_get(ctx, move |_ctx| {
                let c = {
                    let doc = s.dom.borrow();
                    doc.next_element_sibling(node)
                };
                Ok(match c {
                    Some(c) => wrap(&s, _ctx, c),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("previousElementSibling"),
            Some(mk_get(ctx, move |_ctx| {
                let c = {
                    let doc = s.dom.borrow();
                    doc.prev_element_sibling(node)
                };
                Ok(match c {
                    Some(c) => wrap(&s, _ctx, c),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("classList"),
            Some(mk_get(ctx, move |_ctx| Ok(class_list_for(&s, _ctx, node)))),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("style"),
            Some(mk_get(ctx, move |_ctx| Ok(style_object_for(&s, _ctx, node)))),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("dataset"),
            Some(mk_get(ctx, move |_ctx| {
                let mut init = ObjBuilder::new();
                let doc = s.dom.borrow();
                if let Some(el) = doc.element(node) {
                    for (n, v) in &el.attrs {
                        if let Some(rest) = n.strip_prefix("data-") {
                            let key = rest
                                .split('-')
                                .enumerate()
                                .map(|(i, p)| {
                                    if i == 0 {
                                        p.to_string()
                                    } else {
                                        let mut c = p.chars();
                                        match c.next() {
                                            Some(f) => format!("{}{}", f.to_uppercase(), c.as_str()),
                                            None => String::new(),
                                        }
                                    }
                                })
                                .collect::<String>();
                            init.property(jsstr(&key), jsstr(v), Attribute::all());
                        }
                    }
                }
                drop(doc);
                Ok(JsValue::from(init.build(_ctx)))
            })),
            None,
            Attribute::all(),
        );
    }

    // ---- methods
    {
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let name = arg_str(args, 0, ctx);
                let doc = s.dom.borrow();
                Ok(match doc.attr(node, &name) {
                    Some(v) => JsValue::from(jsstr(v)),
                    None => JsValue::null(),
                })
            }),
            js_string!("getAttribute"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let name = arg_str(args, 0, ctx);
                let value = arg_str(args, 1, ctx);
                let mut doc = s.dom.borrow_mut();
                doc.set_attr(node, &name, &value);
                drop(doc);
                s.changed.set(true);
                Ok(JsValue::undefined())
            }),
            js_string!("setAttribute"),
            2,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let name = arg_str(args, 0, ctx);
                let mut doc = s.dom.borrow_mut();
                doc.remove_attr(node, &name);
                drop(doc);
                s.changed.set(true);
                Ok(JsValue::undefined())
            }),
            js_string!("removeAttribute"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let name = arg_str(args, 0, ctx);
                let doc = s.dom.borrow();
                Ok(JsValue::from(doc.attr(node, &name).is_some()))
            }),
            js_string!("hasAttribute"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let child = args.first().and_then(|v| node_of(&s, v, ctx));
                let mut doc = s.dom.borrow_mut();
                if let Some(c) = child {
                    doc.append_child(node, c);
                    drop(doc);
                    s.changed.set(true);
                    Ok(args.first().cloned().unwrap_or(JsValue::undefined()))
                } else {
                    Err(JsNativeError::typ().with_message("appendChild: not a node").into())
                }
            }),
            js_string!("appendChild"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let child = args.first().and_then(|v| node_of(&s, v, ctx));
                let reference = args.get(1).and_then(|v| node_of(&s, v, ctx));
                let mut doc = s.dom.borrow_mut();
                if let Some(c) = child {
                    doc.insert_before(node, c, reference);
                    drop(doc);
                    s.changed.set(true);
                    Ok(args.first().cloned().unwrap_or(JsValue::undefined()))
                } else {
                    Err(JsNativeError::typ().with_message("insertBefore: not a node").into())
                }
            }),
            js_string!("insertBefore"),
            2,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let child = args.first().and_then(|v| node_of(&s, v, ctx));
                let mut doc = s.dom.borrow_mut();
                if let Some(c) = child {
                    doc.remove_child(node, c);
                    drop(doc);
                    s.changed.set(true);
                    Ok(args.first().cloned().unwrap_or(JsValue::undefined()))
                } else {
                    Ok(JsValue::null())
                }
            }),
            js_string!("removeChild"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                if let Some(n) = node_of(&s, args.first().unwrap_or(&JsValue::undefined()), ctx) {
                    let mut doc = s.dom.borrow_mut();
                    doc.detach(n);
                    drop(doc);
                    s.changed.set(true);
                }
                Ok(JsValue::undefined())
            }),
            js_string!("remove"),
            0,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let mut doc = s.dom.borrow_mut();
                for v in args {
                    if let Some(c) = node_of(&s, v, ctx) {
                        doc.append_child(node, c);
                    } else {
                        let text = val_str(v, ctx);
                        let t = doc.create_text(&text);
                        doc.append_child(node, t);
                    }
                }
                drop(doc);
                s.changed.set(true);
                Ok(JsValue::undefined())
            }),
            js_string!("append"),
            0,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let selector = arg_str(args, 0, ctx);
                let res = query_all(&s, node, &selector, false);
                Ok(match res.first() {
                    Some(&n) => wrap(&s, ctx, n),
                    None => JsValue::null(),
                })
            }),
            js_string!("querySelector"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let selector = arg_str(args, 0, ctx);
                let res = query_all(&s, node, &selector, false);
                Ok(wrap_nodes(&s, ctx, &res))
            }),
            js_string!("querySelectorAll"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let tag = arg_str(args, 0, ctx).to_ascii_lowercase();
                let doc = s.dom.borrow();
                let res: Vec<NodeId> = doc
                    .walk(node)
                    .into_iter()
                    .skip(1)
                    .filter(|&n| doc.tag(n) == Some(tag.as_str()) || tag == "*")
                    .collect();
                drop(doc);
                Ok(wrap_nodes(&s, ctx, &res))
            }),
            js_string!("getElementsByTagName"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let class = arg_str(args, 0, ctx);
                let doc = s.dom.borrow();
                let res: Vec<NodeId> = doc
                    .walk(node)
                    .into_iter()
                    .skip(1)
                    .filter(|&n| doc.classes(n).iter().any(|c| *c == class))
                    .collect();
                drop(doc);
                Ok(wrap_nodes(&s, ctx, &res))
            }),
            js_string!("getElementsByClassName"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let selector = arg_str(args, 0, ctx);
                let doc = s.dom.borrow();
                let sels: Vec<_> = crate::css::split_top_level(&selector, ',')
                    .into_iter()
                    .filter_map(|p| parse_selector(&p))
                    .collect();
                let mctx = MatchContext { doc: &doc, hovered: None, active: None, focused: None };
                let mut cur = Some(node);
                while let Some(n) = cur {
                    if doc.is_element(n) && sels.iter().any(|sel| sel.matches(n, &mctx)) {
                        let out = wrap(&s, ctx, n);
                        return Ok(out);
                    }
                    cur = doc.nodes[n].parent;
                }
                Ok(JsValue::null())
            }),
            js_string!("closest"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let selector = arg_str(args, 0, ctx);
                let doc = s.dom.borrow();
                let sels: Vec<_> = crate::css::split_top_level(&selector, ',')
                    .into_iter()
                    .filter_map(|p| parse_selector(&p))
                    .collect();
                let mctx = MatchContext { doc: &doc, hovered: None, active: None, focused: None };
                Ok(JsValue::from(sels.iter().any(|sel| sel.matches(node, &mctx))))
            }),
            js_string!("matches"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let other = args.first().and_then(|v| node_of(&s, v, ctx));
                let doc = s.dom.borrow();
                let mut cur = other;
                while let Some(n) = cur {
                    if n == node {
                        return Ok(JsValue::from(true));
                    }
                    cur = doc.nodes[n].parent;
                }
                Ok(JsValue::from(false))
            }),
            js_string!("contains"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let event = arg_str(args, 0, ctx);
                if let Some(v) = args.get(1) {
                    if let Some(func) = v.as_object().cloned().and_then(JsFunction::from_object) {
                        let idx = push_rooted(ctx, func)?;
                        s.listeners.borrow_mut().push(ListenerEntry {
                            node: Some(node),
                            event,
                            func_index: idx,
                        });
                    }
                }
                Ok(JsValue::undefined())
            }),
            js_string!("addEventListener"),
            2,
        );
        let s = base.clone();
        init.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("removeEventListener"),
            2,
        );
        let s = base.clone();
        init.function(
            mk_method(move |_args, ctx| {
                let _ = dispatch_event(&s, ctx, Some(node), "click");
                Ok(JsValue::undefined())
            }),
            js_string!("click"),
            0,
        );
        let s = base.clone();
        init.function(
            mk_method(move |_args, ctx| {
                let _ = dispatch_event(&s, ctx, Some(node), "focus");
                Ok(JsValue::undefined())
            }),
            js_string!("focus"),
            0,
        );
        let s = base.clone();
        init.function(
            mk_method(move |_args, ctx| {
                let _ = dispatch_event(&s, ctx, Some(node), "blur");
                Ok(JsValue::undefined())
            }),
            js_string!("blur"),
            0,
        );
        init.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("scrollIntoView"),
            0,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let deep = args.first().map(|v| v.to_boolean()).unwrap_or(false);
                let mut doc = s.dom.borrow_mut();
                let n = if deep { doc.deep_clone(node) } else { doc.deep_clone(node) };
                drop(doc);
                Ok(wrap(&s, ctx, n))
            }),
            js_string!("cloneNode"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let html = arg_str(args, 0, ctx);
                if let Some(body) = {
                    let doc = s.dom.borrow();
                    doc.walk(doc.root)
                        .into_iter()
                        .find(|&n| doc.tag(n) == Some("body"))
                } {
                    let mut doc = s.dom.borrow_mut();
                    crate::html::set_inner_html(&mut doc, body, &html);
                    drop(doc);
                    s.changed.set(true);
                }
                let _ = ctx;
                Ok(JsValue::undefined())
            }),
            js_string!("replaceChildrenFromHTML"),
            1,
        );
    }
    JsValue::from(init.build(ctx))
}

fn sibling_of(shared: &Shared, node: NodeId, dir: i32) -> Option<NodeId> {
    let doc = shared.dom.borrow();
    let parent = doc.nodes[node].parent?;
    let kids = &doc.nodes[parent].children;
    let idx = kids.iter().position(|&c| c == node)?;
    if dir > 0 {
        kids.get(idx + 1).copied()
    } else if idx > 0 {
        kids.get(idx - 1).copied()
    } else {
        None
    }
}

// ---------------------------------------------------------------- document

fn wrap_document(shared: &Shared, ctx: &mut Context) -> JsValue {
    let root = shared.dom.borrow().root;
    let s = shared.clone();
    let base = s.clone();
    let mut init = ObjBuilder::new();
    init.property(js_string!("__auroraNode"), root as f64, Attribute::all());
    init.property(js_string!("nodeType"), 9.0, Attribute::all());
    init.property(js_string!("readyState"), js_string!("complete"), Attribute::all());
    init.property(js_string!("characterSet"), js_string!("UTF-8"), Attribute::all());
    init.property(js_string!("compatMode"), js_string!("CSS1Compat"), Attribute::all());
    {
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let id = arg_str(args, 0, ctx);
                let found = {
                    let doc = s.dom.borrow();
                    doc.find_by_id(&id)
                };
                Ok(match found {
                    Some(n) => wrap(&s, ctx, n),
                    None => JsValue::null(),
                })
            }),
            js_string!("getElementById"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let selector = arg_str(args, 0, ctx);
                let root = s.dom.borrow().root;
                let res = query_all(&s, root, &selector, true);
                Ok(match res.first() {
                    Some(&n) => wrap(&s, ctx, n),
                    None => JsValue::null(),
                })
            }),
            js_string!("querySelector"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let selector = arg_str(args, 0, ctx);
                let root = s.dom.borrow().root;
                let res = query_all(&s, root, &selector, true);
                Ok(wrap_nodes(&s, ctx, &res))
            }),
            js_string!("querySelectorAll"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let tag = arg_str(args, 0, ctx).to_ascii_lowercase();
                let doc = s.dom.borrow();
                let res: Vec<NodeId> = doc
                    .walk(doc.root)
                    .into_iter()
                    .filter(|&n| doc.tag(n) == Some(tag.as_str()) || tag == "*")
                    .collect();
                drop(doc);
                Ok(wrap_nodes(&s, ctx, &res))
            }),
            js_string!("getElementsByTagName"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let class = arg_str(args, 0, ctx);
                let doc = s.dom.borrow();
                let res: Vec<NodeId> = doc
                    .walk(doc.root)
                    .into_iter()
                    .filter(|&n| doc.classes(n).iter().any(|c| *c == class))
                    .collect();
                drop(doc);
                Ok(wrap_nodes(&s, ctx, &res))
            }),
            js_string!("getElementsByClassName"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let tag = arg_str(args, 0, ctx);
                let mut doc = s.dom.borrow_mut();
                let n = doc.create_element(&tag);
                drop(doc);
                Ok(wrap(&s, ctx, n))
            }),
            js_string!("createElement"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let text = arg_str(args, 0, ctx);
                let mut doc = s.dom.borrow_mut();
                let n = doc.create_text(&text);
                drop(doc);
                Ok(wrap(&s, ctx, n))
            }),
            js_string!("createTextNode"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let text = arg_str(args, 0, ctx);
                let mut doc = s.dom.borrow_mut();
                let n = doc.add_node(NodeData::Comment(text));
                drop(doc);
                Ok(wrap(&s, ctx, n))
            }),
            js_string!("createComment"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let html = arg_str(args, 0, ctx);
                let body = {
                    let doc = s.dom.borrow();
                    doc.walk(doc.root)
                        .into_iter()
                        .find(|&n| doc.tag(n) == Some("body"))
                };
                if let Some(body) = body {
                    let mut doc = s.dom.borrow_mut();
                    crate::html::append_html(&mut doc, body, &html);
                    drop(doc);
                    s.changed.set(true);
                }
                let _ = ctx;
                Ok(JsValue::undefined())
            }),
            js_string!("write"),
            1,
        );
        let s = base.clone();
        init.function(
            mk_method(move |args, ctx| {
                let event = arg_str(args, 0, ctx);
                if let Some(func) = args.get(1).and_then(|v| v.as_object().cloned().and_then(JsFunction::from_object)) {
                    let idx = push_rooted(ctx, func)?;
                    s.listeners.borrow_mut().push(ListenerEntry {
                        node: Some(s.dom.borrow().root),
                        event,
                        func_index: idx,
                    });
                }
                Ok(JsValue::undefined())
            }),
            js_string!("addEventListener"),
            2,
        );
        init.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("removeEventListener"),
            2,
        );
        let s = base.clone();
        init.accessor(
            js_string!("documentElement"),
            Some(mk_get(ctx, move |ctx| {
                let n = {
                    let doc = s.dom.borrow();
                    doc.walk(doc.root).into_iter().find(|&n| doc.tag(n) == Some("html"))
                };
                Ok(match n {
                    Some(n) => wrap(&s, ctx, n),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("body"),
            Some(mk_get(ctx, move |ctx| {
                let n = {
                    let doc = s.dom.borrow();
                    doc.walk(doc.root).into_iter().find(|&n| doc.tag(n) == Some("body"))
                };
                Ok(match n {
                    Some(n) => wrap(&s, ctx, n),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("head"),
            Some(mk_get(ctx, move |ctx| {
                let n = {
                    let doc = s.dom.borrow();
                    doc.walk(doc.root).into_iter().find(|&n| doc.tag(n) == Some("head"))
                };
                Ok(match n {
                    Some(n) => wrap(&s, ctx, n),
                    None => JsValue::null(),
                })
            })),
            None,
            Attribute::all(),
        );
        let s = base.clone();
        let s_set = base.clone();
        init.accessor(
            js_string!("title"),
            Some(mk_get(ctx, move |_ctx| {
                let doc = s.dom.borrow();
                Ok(JsValue::from(jsstr(&doc.title())))
            })),
            Some(mk_set(ctx, move |v, ctx| {
                let title = val_str(&v, ctx);
                let mut doc = s_set.dom.borrow_mut();
                let existing = doc
                    .walk(doc.root)
                    .into_iter()
                    .find(|&n| doc.tag(n) == Some("title"));
                if let Some(n) = existing {
                    doc.set_text_content(n, &title);
                } else if let Some(head) = doc
                    .walk(doc.root)
                    .into_iter()
                    .find(|&n| doc.tag(n) == Some("head"))
                {
                    let n = doc.create_element("title");
                    let t = doc.create_text(&title);
                    doc.append_child(n, t);
                    doc.append_child(head, n);
                }
                drop(doc);
                s_set.changed.set(true);
                let _ = ctx;
                Ok(())
            })),
            Attribute::all(),
        );
        let s = base.clone();
        init.accessor(
            js_string!("URL"),
            Some(mk_get(ctx, move |_ctx| Ok(JsValue::from(jsstr(&s.url.borrow()))))),
            None,
            Attribute::all(),
        );
        init.accessor(
            js_string!("cookie"),
            Some(mk_get(ctx, |_| Ok(JsValue::from(jsstr(""))))),
            Some(mk_set(ctx, |_v, _| Ok(()))),
            Attribute::all(),
        );
        let s = base.clone();
        let s_set = base.clone();
        init.accessor(
            js_string!("location"),
            Some(mk_get(ctx, move |_ctx| Ok(make_location(&s, _ctx)))),
            Some(mk_set(ctx, move |v, ctx| {
                let href = val_str(&v, ctx);
                *s_set.url.borrow_mut() = href;
                s_set.changed.set(true);
                Ok(())
            })),
            Attribute::all(),
        );
        init.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("close"),
            0,
        );
        init.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("open"),
            0,
        );
    }
    JsValue::from(init.build(ctx))
}

fn location_part(url: &str, part: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let (authority, tail) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (hostname, port) = match authority.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, p),
        _ => (authority, ""),
    };
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, q),
        None => (tail, ""),
    };
    let (path, hash) = match path.split_once('#') {
        Some((p, h)) => (p, h),
        None => (path, ""),
    };
    match part {
        "protocol" => format!("{}:", scheme),
        "host" => authority.to_string(),
        "hostname" => hostname.to_string(),
        "port" => port.to_string(),
        "origin" => {
            if scheme.is_empty() {
                "null".into()
            } else {
                format!("{}://{}", scheme, authority)
            }
        }
        "search" => {
            if query.is_empty() {
                String::new()
            } else {
                format!("?{}", query)
            }
        }
        "hash" => {
            if hash.is_empty() {
                String::new()
            } else {
                format!("#{}", hash)
            }
        }
        _ => path.to_string(),
    }
}

fn make_location(shared: &Shared, ctx: &mut Context) -> JsValue {
    let s = shared.clone();
    let base = s.clone();
    let mut init = ObjBuilder::new();
    let get = mk_get(ctx, move |_ctx| Ok(JsValue::from(jsstr(&s.url.borrow()))));
    let s = shared.clone();
    let set = mk_set(ctx, move |v, ctx| {
        let href = val_str(&v, ctx);
        *s.url.borrow_mut() = href.clone();
        *s.pending_nav.borrow_mut() = Some(href);
        s.changed.set(true);
        let _ = ctx;
        Ok(())
    });
    init.accessor(js_string!("href"), Some(get), Some(set), Attribute::all());
    let s = shared.clone();
    init.accessor(
        js_string!("pathname"),
        Some(mk_get(ctx, move |_ctx| {
            let url = s.url.borrow().clone();
            let p = url
                .find("://")
                .and_then(|i| url[i + 3..].find('/').map(|j| i + 3 + j))
                .map(|i| url[i..].to_string())
                .unwrap_or_else(|| "/".to_string());
            Ok(JsValue::from(jsstr(&p)))
        })),
        None,
        Attribute::all(),
    );
    for prop in ["search", "hash", "protocol", "host", "hostname", "port", "origin"] {
        let s = shared.clone();
        let name = prop.to_string();
        init.accessor(
            js_string!(prop),
            Some(mk_get(ctx, move |_ctx| {
                let url = s.url.borrow().clone();
                let val = location_part(&url, &name);
                Ok(JsValue::from(jsstr(&val)))
            })),
            None,
            Attribute::all(),
        );
    }
    let st = shared.clone();
    init.function(
        mk_method(move |_args, _ctx| Ok(JsValue::from(jsstr(&st.url.borrow())))),
        js_string!("toString"),
        0,
    );
    let s = shared.clone();
    init.function(
        mk_method(move |_args, _ctx| {
            s.changed.set(true);
            Ok(JsValue::undefined())
        }),
        js_string!("reload"),
        0,
    );
    let s = shared.clone();
    init.function(
        mk_method(move |args, ctx| {
            let href = arg_str(args, 0, ctx);
            *s.url.borrow_mut() = href.clone();
            *s.pending_nav.borrow_mut() = Some(href);
            s.changed.set(true);
            Ok(JsValue::undefined())
        }),
        js_string!("assign"),
        1,
    );
    let sr = shared.clone();
    init.function(
        mk_method(move |args, ctx| {
            let href = arg_str(args, 0, ctx);
            *sr.url.borrow_mut() = href.clone();
            *sr.pending_nav.borrow_mut() = Some(href);
            sr.changed.set(true);
            let _ = ctx;
            Ok(JsValue::undefined())
        }),
        js_string!("replace"),
        1,
    );
    JsValue::from(init.build(ctx))
}

// ------------------------------------------------------------------- init

impl JsRuntime {
    pub fn new(dom: Rc<RefCell<Document>>, base_url: &str, viewport: (f64, f64)) -> JsRuntime {
        let mut ctx = Context::default();
        let shared = Shared {
            dom,
            listeners: Rc::new(RefCell::new(Vec::new())),
            timers: Rc::new(RefCell::new(Vec::new())),
            storage: Rc::new(RefCell::new(HashMap::new())),
            url: Rc::new(RefCell::new(base_url.to_string())),
            pending_nav: Rc::new(RefCell::new(None)),
            changed: Rc::new(Cell::new(false)),
            prevented: Rc::new(Cell::new(false)),
            stopped: Rc::new(Cell::new(false)),
            inner_size: viewport,
        };
        let fns = JsArray::new(&mut ctx);
        let _ = ctx.register_global_property(
            js_string!("__auroraFns"),
            fns,
            Attribute::all(),
        );
        let doc_val = wrap_document(&shared, &mut ctx);
        let _ = ctx.register_global_property(js_string!("document"), doc_val, Attribute::all());
        let _ = ctx.register_global_property(
            js_string!("window"),
            ctx.global_object(),
            Attribute::all(),
        );
        let _ = ctx.register_global_property(
            js_string!("self"),
            ctx.global_object(),
            Attribute::all(),
        );
        let _ = ctx.register_global_property(
            js_string!("innerWidth"),
            shared.inner_size.0,
            Attribute::all(),
        );
        let _ = ctx.register_global_property(
            js_string!("innerHeight"),
            shared.inner_size.1,
            Attribute::all(),
        );
        let _ = ctx.register_global_property(js_string!("devicePixelRatio"), 1.0, Attribute::all());
        let _ = ctx.register_global_property(js_string!("scrollX"), 0.0, Attribute::all());
        let _ = ctx.register_global_property(js_string!("scrollY"), 0.0, Attribute::all());

        let mut rt = JsRuntime { ctx, shared, errors: Vec::new() };
        rt.install_globals();
        rt
    }

    fn install_globals(&mut self) {
        let ctx = &mut self.ctx;
        let shared = self.shared.clone();
        let loc = make_location(&shared, ctx);
        let _ = ctx.register_global_property(js_string!("location"), loc, Attribute::all());

        // console
        let mut console = ObjBuilder::new();
        console.function(
            mk_method(move |args, ctx| {
                let msg = args
                    .iter()
                    .map(|v| val_str(v, ctx))
                    .collect::<Vec<_>>()
                    .join(" ");
                eprintln!("[console] {}", msg);
                Ok(JsValue::undefined())
            }),
            js_string!("log"),
            0,
        );
        console.function(
            mk_method(move |args, ctx| {
                let msg = args
                    .iter()
                    .map(|v| val_str(v, ctx))
                    .collect::<Vec<_>>()
                    .join(" ");
                eprintln!("[console:warn] {}", msg);
                Ok(JsValue::undefined())
            }),
            js_string!("warn"),
            0,
        );
        console.function(
            mk_method(move |args, ctx| {
                let msg = args
                    .iter()
                    .map(|v| val_str(v, ctx))
                    .collect::<Vec<_>>()
                    .join(" ");
                eprintln!("[console:error] {}", msg);
                Ok(JsValue::undefined())
            }),
            js_string!("error"),
            0,
        );
        console.function(
            mk_method(move |args, ctx| {
                let msg = args
                    .iter()
                    .map(|v| val_str(v, ctx))
                    .collect::<Vec<_>>()
                    .join(" ");
                eprintln!("[console] {}", msg);
                Ok(JsValue::undefined())
            }),
            js_string!("info"),
            0,
        );
        let console_obj = JsValue::from(console.build(ctx));
        let _ = ctx.register_global_property(js_string!("console"), console_obj, Attribute::all());

        // timers
        let s = shared.clone();
        let nf = mk_method(move |args, ctx| {
            if let Some(func) = args.first().and_then(|v| v.as_object().cloned().and_then(JsFunction::from_object)) {
                let idx = push_rooted(ctx, func)?;
                s.timers.borrow_mut().push(idx);
            }
            Ok(JsValue::from(0.0))
        });
        let _ = ctx.register_global_builtin_callable(js_string!("setTimeout"), 2, nf);
        let s = shared.clone();
        let nf = mk_method(move |args, ctx| {
            if let Some(func) = args.first().and_then(|v| v.as_object().cloned().and_then(JsFunction::from_object)) {
                let idx = push_rooted(ctx, func)?;
                s.timers.borrow_mut().push(idx);
            }
            Ok(JsValue::from(0.0))
        });
        let _ = ctx.register_global_builtin_callable(js_string!("setInterval"), 2, nf);
        let _ = ctx.register_global_builtin_callable(
            js_string!("requestAnimationFrame"),
            1,
            {
                let s = shared.clone();
                mk_method(move |args, ctx| {
                    if let Some(func) = args.first().and_then(|v| v.as_object().cloned().and_then(JsFunction::from_object)) {
                        let idx = push_rooted(ctx, func)?;
                        s.timers.borrow_mut().push(idx);
                    }
                    Ok(JsValue::from(0.0))
                })
            },
        );
        let _ = ctx.register_global_builtin_callable(
            js_string!("clearTimeout"),
            1,
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
        );
        let _ = ctx.register_global_builtin_callable(
            js_string!("clearInterval"),
            1,
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
        );
        let _ = ctx.register_global_builtin_callable(
            js_string!("alert"),
            1,
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
        );
        let _ = ctx.register_global_builtin_callable(
            js_string!("confirm"),
            1,
            mk_method(move |_args, _ctx| Ok(JsValue::from(false))),
        );
        let _ = ctx.register_global_builtin_callable(
            js_string!("prompt"),
            2,
            mk_method(move |_args, _ctx| Ok(JsValue::null())),
        );
        let _ = ctx.register_global_builtin_callable(
            js_string!("scrollTo"),
            2,
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
        );
        let _ = ctx.register_global_builtin_callable(
            js_string!("scrollBy"),
            2,
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
        );

        // window addEventListener
        let s = shared.clone();
        let nf = mk_method(move |args, ctx| {
            let event = arg_str(args, 0, ctx);
            if let Some(func) = args.get(1).and_then(|v| v.as_object().cloned().and_then(JsFunction::from_object)) {
                let idx = push_rooted(ctx, func)?;
                s.listeners.borrow_mut().push(ListenerEntry {
                    node: None,
                    event,
                    func_index: idx,
                });
            }
            Ok(JsValue::undefined())
        });
        let _ = ctx.register_global_builtin_callable(js_string!("addEventListener"), 2, nf);
        let _ = ctx.register_global_builtin_callable(
            js_string!("removeEventListener"),
            2,
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
        );

        // navigator
        let mut nav = ObjBuilder::new();
        nav.property(
            js_string!("userAgent"),
            js_string!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Aurora/1.0 Safari/537.36"),
            Attribute::all(),
        );
        nav.property(js_string!("language"), js_string!("en-US"), Attribute::all());
        nav.property(js_string!("platform"), js_string!("Win32"), Attribute::all());
        nav.property(js_string!("onLine"), true, Attribute::all());
        let nav_val = JsValue::from(nav.build(ctx));
        let _ = ctx.register_global_property(js_string!("navigator"), nav_val, Attribute::all());

        // localStorage
        let s = shared.clone();
        let mut ls = ObjBuilder::new();
        ls.function(
            mk_method(move |args, ctx| {
                let key = arg_str(args, 0, ctx);
                Ok(match s.storage.borrow().get(&key) {
                    Some(v) => JsValue::from(jsstr(v)),
                    None => JsValue::null(),
                })
            }),
            js_string!("getItem"),
            1,
        );
        let s = shared.clone();
        ls.function(
            mk_method(move |args, ctx| {
                let k = arg_str(args, 0, ctx);
                let v = arg_str(args, 1, ctx);
                s.storage.borrow_mut().insert(k, v);
                Ok(JsValue::undefined())
            }),
            js_string!("setItem"),
            2,
        );
        let s = shared.clone();
        ls.function(
            mk_method(move |args, ctx| {
                let k = arg_str(args, 0, ctx);
                s.storage.borrow_mut().remove(&k);
                Ok(JsValue::undefined())
            }),
            js_string!("removeItem"),
            1,
        );
        let s = shared.clone();
        ls.function(
            mk_method(move |args, ctx| {
                let k = arg_str(args, 0, ctx);
                Ok(JsValue::from(s.storage.borrow().contains_key(&k)))
            }),
            js_string!("hasItem"),
            1,
        );
        let ls_val = JsValue::from(ls.build(ctx));
        let _ = ctx.register_global_property(js_string!("localStorage"), ls_val.clone(), Attribute::all());
        let _ = ctx.register_global_property(js_string!("sessionStorage"), ls_val, Attribute::all());

        // performance
        let mut perf = ObjBuilder::new();
        perf.function(
            mk_method(move |_args, _ctx| Ok(JsValue::from(0.0))),
            js_string!("now"),
            0,
        );
        let perf_val = JsValue::from(perf.build(ctx));
        let _ = ctx.register_global_property(js_string!("performance"), perf_val, Attribute::all());

        // fetch (synchronous network + resolved promises)
        let s = shared.clone();
        let nf = mk_method(move |args, ctx| {
            let url = arg_str(args, 0, ctx);
            let base = s.url.borrow().clone();
            let abs = crate::net::resolve_url(&base, &url);
            let (status, status_text, text, content_type, final_url) =
                match crate::net::fetch(&abs) {
                    Ok(resp) => {
                        let ct = resp.content_type.clone();
                        let status = resp.status;
                        let status_text = if status == 200 { "OK" } else { "" };
                        let text = crate::net::decode_body(&resp);
                        (status, status_text.to_string(), text, ct, resp.url)
                    }
                    Err(e) => (0, e, String::new(), String::new(), abs.clone()),
                };
            let response = make_response(
                &s,
                ctx,
                status,
                &status_text,
                &final_url,
                &text,
                &content_type,
            );
            Ok(JsValue::from(JsPromise::resolve(response, ctx)))
        });
        let _ = ctx.register_global_builtin_callable(js_string!("fetch"), 1, nf);

        // matchMedia
        let s = shared.clone();
        let nf = mk_method(move |args, ctx| {
            let mut init = ObjBuilder::new();
            init.property(js_string!("matches"), false, Attribute::all());
            init.property(js_string!("media"), jsstr(&arg_str(args, 0, ctx)), Attribute::all());
            let _s = s.clone();
            init.function(
                mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
                js_string!("addListener"),
                1,
            );
            init.function(
                mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
                js_string!("removeListener"),
                1,
            );
            init.function(
                mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
                js_string!("addEventListener"),
                2,
            );
            Ok(JsValue::from(init.build(ctx)))
        });
        let _ = ctx.register_global_builtin_callable(js_string!("matchMedia"), 1, nf);

        // getComputedStyle stub
        let s = shared.clone();
        let nf = mk_method(move |args, ctx| {
            let mut init = ObjBuilder::new();
            let _s = s.clone();
            init.function(
                mk_method(move |_args, _ctx| Ok(JsValue::from(jsstr("")))),
                js_string!("getPropertyValue"),
                1,
            );
            Ok(JsValue::from(init.build(ctx)))
        });
        let _ = ctx.register_global_builtin_callable(js_string!("getComputedStyle"), 1, nf);
    }

    pub fn run_scripts(&mut self, scripts: &[ScriptItem]) {
        self.ctx
            .runtime_limits_mut()
            .set_loop_iteration_limit(5_000_000);
        // Minimal DOM/Web API shims for feature detection and common utilities.
        let _ = self.ctx.eval(Source::from_bytes(BOOTSTRAP_JS));
        let mut total_bytes = 0usize;
        for s in scripts {
            if let Some(url) = &s.url {
                let low = url.to_ascii_lowercase();
                if low.contains("polyfill")
                    || low.contains("googletagmanager")
                    || low.contains("google-analytics")
                    || low.contains("doubleclick")
                    || low.contains("hotjar")
                    || low.contains("facebook.net")
                    || low.contains("ntv.io")
                    || low.contains("googlesyndication")
                    || low.contains("adservice")
                    || low.contains("doubleclick.net")
                    || low.contains("prebid")
                    || low.contains("criteo")
                    || low.contains("taboola")
                    || low.contains("outbrain")
                    || low.contains("amazon-adsystem")
                    || low.contains("scorecardresearch")
                    || low.contains("quantserve")
                    || low.contains("chartbeat")
                    || low.contains("parsely")
                    || low.contains("optimizely")
                {
                    continue;
                }
            }
            let head = {
                let n = s.text.len().min(4096);
                s.text[..n].to_ascii_lowercase()
            };
            if head.contains("core-js") || head.contains("regenerator-runtime") {
                continue;
            }
            let len = s.text.len();
            total_bytes += len;
            if len > 4_000_000 || total_bytes > 16_000_000 {
                self.errors
                    .push(format!("script skipped ({} bytes) to keep the JS heap bounded", len));
                continue;
            }
            boa_engine::gc::force_collect();
            if std::env::var("AURORA_DEBUG_JS").is_ok() {
                eprintln!(
                    "eval script {} bytes url={:?}",
                    len,
                    s.url.as_deref().unwrap_or("<inline>")
                );
            }
            let src = if let Some(url) = &s.url {
                Source::from_bytes(&s.text).with_path(std::path::Path::new(url))
            } else {
                Source::from_bytes(&s.text)
            };
            match self.ctx.eval(src) {
                Ok(_) => {}
                Err(e) => {
                    let msg = format!("JavaScript error: {}", e);
                    eprintln!("{}", msg);
                    self.errors.push(msg);
                }
            }
        }
        let _ = dispatch_event(&self.shared, &mut self.ctx, None, "DOMContentLoaded");
        self.run_timers();
        self.ctx.run_jobs();
        let _ = dispatch_event(&self.shared, &mut self.ctx, None, "load");
        self.ctx.run_jobs();
        self.shared.changed.set(true);
    }

    fn run_timers(&mut self) {
        for _ in 0..200 {
            let idx = {
                let mut t = self.shared.timers.borrow_mut();
                match t.pop() {
                    Some(i) => i,
                    None => break,
                }
            };
            if let Some(func) = get_rooted(&mut self.ctx, idx) {
                let _ = func.call(&JsValue::undefined(), &[], &mut self.ctx);
            }
            self.ctx.run_jobs();
        }
    }

    pub fn dispatch(&mut self, node: Option<NodeId>, event: &str) -> bool {
        match dispatch_event(&self.shared, &mut self.ctx, node, event) {
            Ok(prevented) => prevented,
            Err(_) => false,
        }
    }

    pub fn take_changed(&self) -> bool {
        let c = self.shared.changed.get();
        self.shared.changed.set(false);
        c
    }

    pub fn take_pending_navigation(&mut self) -> Option<String> {
        self.shared.pending_nav.borrow_mut().take()
    }

    pub fn tick_timers(&mut self) {
        let pending = !self.shared.timers.borrow().is_empty();
        if pending {
            self.run_timers();
            self.ctx.run_jobs();
        }
    }
}
