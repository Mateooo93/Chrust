p = 'src/layout.rs'
s = open(p, encoding='utf-8').read()

# helper on LayoutEngine
s = s.replace('''    fn resolve_margin(&self, l: Length, basis: f32) -> f32 {''', '''    /// Basis for percentage heights: the nearest ancestor with an explicit
    /// height (html/body use the viewport). This mirrors how browsers resolve
    /// percentages against a definite containing-block height.
    fn height_basis_for(&self, node: NodeId) -> Option<f32> {
        if matches!(self.doc.tag(node), Some("html") | Some("body")) {
            return Some(self.viewport.1);
        }
        let mut cur = self.doc.nodes[node].parent;
        while let Some(p) = cur {
            if self.doc.is_element(p) {
                let tag = self.doc.tag(p).unwrap_or("");
                if tag == "html" || tag == "body" {
                    return Some(self.viewport.1);
                }
                if let Some(style) = self.styles.get(p) {
                    match style.height {
                        Length::Px(v) => return Some(v),
                        Length::Percent(_) => return None,
                        Length::Auto => {}
                    }
                }
            }
            cur = self.doc.nodes[p].parent;
        }
        None
    }

    fn resolve_margin(&self, l: Length, basis: f32) -> f32 {''')

# replaced element height percent
s = s.replace('''                Length::Percent(pct) => (self.viewport.1 * pct).max(0.0),
                Length::Auto => ih,''', '''                Length::Percent(pct) => self
                    .height_basis_for(b.node)
                    .map(|h| h * pct)
                    .unwrap_or(ih),
                Length::Auto => ih,''')

# min/max heights should use a height basis
s = s.replace('''            if let Some(v) = crate::css::resolve_length(style.min_height, basis) {
                h = h.max(v);
            }
            if let Some(v) = crate::css::resolve_length(style.max_height, basis) {
                h = h.min(v);
            }
            forced_h = Some(h.max(0.0));''', '''            let hb = self.height_basis_for(b.node);
            if let Some(v) = resolve_height_len(style.min_height, hb) {
                h = h.max(v);
            }
            if let Some(v) = resolve_height_len(style.max_height, hb) {
                h = h.min(v);
            }
            forced_h = Some(h.max(0.0));''')

# explicit block height percent
s = s.replace('''        let height_value = match style.height {
            Length::Px(v) => Some(v),
            Length::Percent(p) => Some(self.viewport.1 * p),
            Length::Auto => None,
        };''', '''        let hb = self.height_basis_for(b.node);
        let height_value = match style.height {
            Length::Px(v) => Some(v),
            Length::Percent(p) => hb.map(|h| h * p),
            Length::Auto => None,
        };''')

# block min/max height resolution
s = s.replace('''        if let Some(v) = crate::css::resolve_length(style.min_height, basis) {
            content_h = content_h.max(v);
        }
        if let Some(v) = crate::css::resolve_length(style.max_height, basis) {
            content_h = content_h.min(v);
        }''', '''        if let Some(v) = resolve_height_len(style.min_height, hb) {
            content_h = content_h.max(v);
        }
        if let Some(v) = resolve_height_len(style.max_height, hb) {
            content_h = content_h.min(v);
        }''')

# flex row explicit cross height percent
s = s.replace('''            } else if let Length::Percent(p) = b.style.height {
                cross = self.viewport.1 * p;
            }''', '''            } else if let Length::Percent(p) = b.style.height {
                if let Some(h) = self.height_basis_for(b.node) {
                    cross = h * p;
                }
            }''')

# flex column explicit height percent
s = s.replace('''            let explicit_h = match b.style.height {
                Length::Px(v) => Some(v),
                Length::Percent(p) => Some(self.viewport.1 * p),
                Length::Auto => None,
            };''', '''            let hb = self.height_basis_for(b.node);
            let explicit_h = match b.style.height {
                Length::Px(v) => Some(v),
                Length::Percent(p) => hb.map(|h| h * p),
                Length::Auto => None,
            };''')

# free helper for min/max height
s = s.replace('''fn max_content_bottom(b: &LayoutBox, max_bottom: &mut f32, inside_fixed: bool) -> f32 {''', '''fn resolve_height_len(l: Length, basis: Option<f32>) -> Option<f32> {
    match l {
        Length::Px(v) => Some(v),
        Length::Percent(p) => basis.map(|b| b * p),
        Length::Auto => None,
    }
}

fn max_content_bottom(b: &LayoutBox, max_bottom: &mut f32, inside_fixed: bool) -> f32 {''')

open(p, 'w', encoding='utf-8').write(s)
print('patched heights')
