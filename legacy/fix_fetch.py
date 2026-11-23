p = 'src/js.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''use boa_engine::object::builtins::{JsArray, JsFunction};''',
              '''use boa_engine::object::builtins::{JsArray, JsFunction, JsPromise};''')
s = s.replace('''        // matchMedia''', '''        // fetch (synchronous network + resolved promises)
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

        // matchMedia''')
s = s.replace('''        for s in scripts {''', '''        for s in scripts {''')
# Ensure run_jobs after evaluations
s = s.replace('''        let _ = dispatch_event(&self.shared, &mut self.ctx, None, "DOMContentLoaded");
        self.run_timers();
        let _ = dispatch_event(&self.shared, &mut self.ctx, None, "load");
        self.shared.changed.set(true);''', '''        let _ = dispatch_event(&self.shared, &mut self.ctx, None, "DOMContentLoaded");
        self.run_timers();
        self.ctx.run_jobs();
        let _ = dispatch_event(&self.shared, &mut self.ctx, None, "load");
        self.ctx.run_jobs();
        self.shared.changed.set(true);''')
s = s.replace('''            if let Some(func) = get_rooted(&mut self.ctx, idx) {
                let _ = func.call(&JsValue::undefined(), &[], &mut self.ctx);
            }''', '''            if let Some(func) = get_rooted(&mut self.ctx, idx) {
                let _ = func.call(&JsValue::undefined(), &[], &mut self.ctx);
            }
            self.ctx.run_jobs();''')
# add make_response helper before // ----------------------------------------------------------------- wrappers
marker = '''// ----------------------------------------------------------------- wrappers'''
helper = r'''fn make_response(
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

'''
s = s.replace(marker, helper + marker)
open(p, 'w', encoding='utf-8').write(s)
print('fetch added')
