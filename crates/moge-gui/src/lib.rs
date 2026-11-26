//! GUI layout (Roblox UDim2 / AnchorPoint / UIListLayout ...), hit testing, drag/resize math and snapping.
use moge_core::{Id, Tree, Value};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect { x, y, w: (r - x).max(0.0), h: (b - y).max(0.0) }
    }
}

#[derive(Clone, Debug)]
pub struct GuiNode {
    pub id: Id,
    pub rect: Rect,
    /// Visible area after ancestors with ClipsDescendants.
    pub clip: Rect,
    pub rotation: f32,
    pub depth: u32,
}

pub fn is_gui_object(class: &str) -> bool {
    matches!(
        class,
        "Frame" | "TextLabel" | "TextButton" | "TextBox" | "ImageLabel" | "ImageButton" | "ScrollingFrame" | "ViewportFrame" | "CanvasGroup" | "VideoFrame"
    )
}

pub fn is_gui_root(class: &str) -> bool {
    matches!(class, "ScreenGui")
}

fn udim2(tree: &Tree, id: Id, name: &str) -> (f32, f32, f32, f32) {
    match tree.get_prop(id, name) {
        Some(Value::UDim2 { sx, ox, sy, oy }) => (*sx, *ox as f32, *sy, *oy as f32),
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}
fn udim(tree: &Tree, id: Id, name: &str) -> (f32, f32) {
    match tree.get_prop(id, name) {
        Some(Value::UDim(s, o)) => (*s, *o as f32),
        _ => (0.0, 0.0),
    }
}
fn flt(tree: &Tree, id: Id, name: &str, d: f32) -> f32 {
    match tree.get_prop(id, name) {
        Some(Value::Float(f)) => *f as f32,
        Some(Value::Int(i)) => *i as f32,
        _ => d,
    }
}
fn enm(tree: &Tree, id: Id, name: &str, d: u32) -> u32 {
    match tree.get_prop(id, name) {
        Some(Value::Enum(e)) => *e,
        _ => d,
    }
}
fn flag(tree: &Tree, id: Id, name: &str, d: bool) -> bool {
    match tree.get_prop(id, name) {
        Some(Value::Bool(b)) => *b,
        _ => d,
    }
}
fn v2(tree: &Tree, id: Id, name: &str, d: [f32; 2]) -> [f32; 2] {
    match tree.get_prop(id, name) {
        Some(Value::Vector2(v)) => *v,
        _ => d,
    }
}

fn modifier(tree: &Tree, parent: Id, class: &str) -> Option<Id> {
    tree.find_first_child_of_class(parent, class)
}

/// Content rectangle of `parent` after UIPadding.
fn content_rect(tree: &Tree, parent: Id, r: Rect) -> Rect {
    let Some(p) = modifier(tree, parent, "UIPadding") else { return r };
    let l = udim(tree, p, "PaddingLeft");
    let rt = udim(tree, p, "PaddingRight");
    let t = udim(tree, p, "PaddingTop");
    let b = udim(tree, p, "PaddingBottom");
    let left = l.0 * r.w + l.1;
    let right = rt.0 * r.w + rt.1;
    let top = t.0 * r.h + t.1;
    let bottom = b.0 * r.h + b.1;
    Rect { x: r.x + left, y: r.y + top, w: (r.w - left - right).max(0.0), h: (r.h - top - bottom).max(0.0) }
}

fn own_size(tree: &Tree, id: Id, c: Rect) -> (f32, f32) {
    let (sx, ox, sy, oy) = udim2(tree, id, "Size");
    let (mut w, mut h) = (sx * c.w + ox, sy * c.h + oy);
    if let Some(a) = modifier(tree, id, "UIAspectRatioConstraint") {
        let ratio = flt(tree, a, "AspectRatio", 1.0).max(0.0001);
        let ty = enm(tree, a, "AspectType", 0);
        let dominant_w = enm(tree, a, "DominantAxis", 0) == 0;
        if ty == 0 {
            if w / h.max(0.0001) > ratio { w = h * ratio } else { h = w / ratio }
        } else if dominant_w {
            h = w / ratio
        } else {
            w = h * ratio
        }
    }
    if let Some(s) = modifier(tree, id, "UISizeConstraint") {
        let mn = v2(tree, s, "MinSize", [0.0, 0.0]);
        let mx = v2(tree, s, "MaxSize", [f32::INFINITY, f32::INFINITY]);
        w = w.clamp(mn[0], mx[0].max(mn[0]));
        h = h.clamp(mn[1], mx[1].max(mn[1]));
    }
    if let Some(s) = modifier(tree, id, "UIScale") {
        let k = flt(tree, s, "Scale", 1.0);
        w *= k;
        h *= k;
    }
    (w.max(0.0), h.max(0.0))
}

fn sorted_children(tree: &Tree, parent: Id, by_layout_order: Option<u32>) -> Vec<Id> {
    let mut kids: Vec<Id> = tree
        .node(parent)
        .map(|n| n.children.iter().copied().filter(|c| tree.node(*c).map(|n| is_gui_object(&n.class)).unwrap_or(false)).collect())
        .unwrap_or_default();
    if let Some(order) = by_layout_order {
        if order == 2 {
            kids.sort_by_key(|k| match tree.get_prop(*k, "LayoutOrder") {
                Some(Value::Int(i)) => *i,
                _ => 0,
            });
        } else if order == 0 {
            kids.sort_by(|a, b| tree.name(*a).cmp(&tree.name(*b)));
        }
    }
    kids
}

/// Absolute rectangles for the children of `parent` (rect `pr`), in child order.
fn place_children(tree: &Tree, parent: Id, pr: Rect) -> Vec<(Id, Rect)> {
    let c = content_rect(tree, parent, pr);
    let mut out = Vec::new();
    if let Some(l) = modifier(tree, parent, "UIListLayout") {
        let kids = sorted_children(tree, parent, Some(enm(tree, l, "SortOrder", 0)));
        let vertical = enm(tree, l, "FillDirection", 1) == 1;
        let pad = udim(tree, l, "Padding");
        let gap = pad.0 * if vertical { c.h } else { c.w } + pad.1;
        let sizes: Vec<(f32, f32)> = kids.iter().map(|k| own_size(tree, *k, c)).collect();
        let main: f32 = sizes.iter().map(|s| if vertical { s.1 } else { s.0 }).sum::<f32>() + gap * kids.len().saturating_sub(1) as f32;
        let ha = enm(tree, l, "HorizontalAlignment", 0);
        let va = enm(tree, l, "VerticalAlignment", 1);
        let mut cursor = if vertical {
            match va {
                0 => c.y + (c.h - main) / 2.0,
                2 => c.bottom() - main,
                _ => c.y,
            }
        } else {
            match ha {
                1 => c.x + (c.w - main) / 2.0,
                2 => c.right() - main,
                _ => c.x,
            }
        };
        for (k, (w, h)) in kids.iter().zip(sizes) {
            let r = if vertical {
                let x = match ha {
                    1 => c.x + (c.w - w) / 2.0,
                    2 => c.right() - w,
                    _ => c.x,
                };
                Rect::new(x, cursor, w, h)
            } else {
                let y = match va {
                    0 => c.y + (c.h - h) / 2.0,
                    2 => c.bottom() - h,
                    _ => c.y,
                };
                Rect::new(cursor, y, w, h)
            };
            cursor += gap + if vertical { h } else { w };
            out.push((*k, r));
        }
        return out;
    }
    if let Some(g) = modifier(tree, parent, "UIGridLayout") {
        let kids = sorted_children(tree, parent, Some(enm(tree, g, "SortOrder", 0)));
        let (csx, csox, csy, csoy) = udim2(tree, g, "CellSize");
        let (psx, psox, psy, psoy) = udim2(tree, g, "CellPadding");
        let (cw, ch) = (csx * c.w + csox, csy * c.h + csoy);
        let (pw, ph) = (psx * c.w + psox, psy * c.h + psoy);
        let per_row = (((c.w + pw) / (cw + pw).max(1.0)).floor() as usize).max(1);
        for (i, k) in kids.iter().enumerate() {
            let (col, row) = (i % per_row, i / per_row);
            out.push((*k, Rect::new(c.x + col as f32 * (cw + pw), c.y + row as f32 * (ch + ph), cw, ch)));
        }
        return out;
    }
    for k in sorted_children(tree, parent, None) {
        let (w, h) = own_size(tree, k, c);
        let (px, pox, py, poy) = udim2(tree, k, "Position");
        let a = v2(tree, k, "AnchorPoint", [0.0, 0.0]);
        out.push((k, Rect::new(c.x + px * c.w + pox - a[0] * w, c.y + py * c.h + poy - a[1] * h, w, h)));
    }
    out
}

/// Lays out a ScreenGui (or any GUI object) for a screen of `screen` pixels.
/// Output is in paint order: a node, then its whole subtree, then the next sibling (siblings by ZIndex).
pub fn layout(tree: &Tree, root: Id, screen: (f32, f32)) -> Vec<GuiNode> {
    let mut out = Vec::new();
    let Some(n) = tree.node(root) else { return out };
    if is_gui_root(&n.class) && !flag(tree, root, "Enabled", true) {
        return out;
    }
    let base = Rect::new(0.0, 0.0, screen.0, screen.1);
    // Entries still to emit: (id, rect, clip applied to the node, clip inherited by its children, depth)
    type Item = (Id, Rect, Rect, Rect, u32);
    let push_children = |stack: &mut Vec<Item>, parent: Id, pr: Rect, clip: Rect, depth: u32| {
        let mut placed = place_children(tree, parent, pr);
        placed.retain(|(k, _)| flag(tree, *k, "Visible", true));
        placed.sort_by_key(|(k, _)| match tree.get_prop(*k, "ZIndex") {
            Some(Value::Int(z)) => *z,
            _ => 1,
        });
        for (k, r) in placed.into_iter().rev() {
            let visible = r.intersect(&clip);
            let clips = flag(tree, k, "ClipsDescendants", false) || tree.node(k).map(|n| n.class == "ScrollingFrame").unwrap_or(false);
            stack.push((k, r, visible, if clips { visible } else { clip }, depth));
        }
    };
    let mut stack: Vec<Item> = Vec::new();
    push_children(&mut stack, root, base, base, 0);
    while let Some((id, rect, clip, child_clip, depth)) = stack.pop() {
        out.push(GuiNode { id, rect, clip, rotation: flt(tree, id, "Rotation", 0.0), depth });
        push_children(&mut stack, id, rect, child_clip, depth + 1);
    }
    out
}

/// Topmost GUI object under the point (honors clipping).
pub fn hit_test(nodes: &[GuiNode], x: f32, y: f32) -> Option<Id> {
    nodes.iter().rev().find(|n| n.clip.contains(x, y) && n.rect.contains(x, y)).map(|n| n.id)
}

// ------------------------------------------------------------------ editing math

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Movement changes the offset component (pixels).
    Offset,
    /// Movement changes the scale component (percent of the parent).
    Scale,
}

fn bump(scale: f32, offset: f32, delta_px: f32, parent_dim: f32, mode: Mode) -> (f32, f32) {
    match mode {
        Mode::Offset => (scale, offset + delta_px),
        Mode::Scale => (scale + if parent_dim.abs() > 1e-6 { delta_px / parent_dim } else { 0.0 }, offset),
    }
}

pub fn move_udim2(pos: (f32, f32, f32, f32), dx: f32, dy: f32, parent: (f32, f32), mode: Mode) -> (f32, f32, f32, f32) {
    let (sx, ox) = bump(pos.0, pos.1, dx, parent.0, mode);
    let (sy, oy) = bump(pos.2, pos.3, dy, parent.1, mode);
    (sx, ox, sy, oy)
}

/// Resizes by dragging a handle. `handle` is (-1|0|1, -1|0|1): the edge(s) being dragged. Returns (position, size).
pub fn resize_udim2(
    pos: (f32, f32, f32, f32),
    size: (f32, f32, f32, f32),
    anchor: [f32; 2],
    handle: (i8, i8),
    dx: f32,
    dy: f32,
    parent: (f32, f32),
    mode: Mode,
    current: (f32, f32),
    min_px: f32,
) -> ((f32, f32, f32, f32), (f32, f32, f32, f32)) {
    let axis = |h: i8, d: f32, a: f32, cur: f32, pd: f32, p: &mut (f32, f32), s: &mut (f32, f32)| {
        if h == 0 {
            return;
        }
        // Dragging the near edge shrinks by d; the far edge grows by d.
        let grow = if h < 0 { -d } else { d };
        let grow = grow.max(min_px - cur);
        let d_eff = if h < 0 { -grow } else { grow };
        *s = bump(s.0, s.1, grow, pd, mode);
        let anchor_shift = if h < 0 { d_eff * (1.0 - a) } else { d_eff * a };
        *p = bump(p.0, p.1, anchor_shift, pd, mode);
    };
    let (mut px, mut sx) = ((pos.0, pos.1), (size.0, size.1));
    let (mut py, mut sy) = ((pos.2, pos.3), (size.2, size.3));
    axis(handle.0, dx, anchor[0], current.0, parent.0, &mut px, &mut sx);
    axis(handle.1, dy, anchor[1], current.1, parent.1, &mut py, &mut sy);
    ((px.0, px.1, py.0, py.1), (sx.0, sx.1, sy.0, sy.1))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Guide {
    Vertical(f32),
    Horizontal(f32),
}

/// Snaps a moving rectangle to the edges/centers of siblings and the parent. Returns the correction to add
/// to the drag delta plus the guide lines to draw.
pub fn snap_to_guides(moving: Rect, others: &[Rect], parent: Rect, threshold: f32) -> ((f32, f32), Vec<Guide>) {
    let mut xs: Vec<f32> = vec![parent.x, parent.x + parent.w / 2.0, parent.right()];
    let mut ys: Vec<f32> = vec![parent.y, parent.y + parent.h / 2.0, parent.bottom()];
    for o in others {
        xs.extend([o.x, o.x + o.w / 2.0, o.right()]);
        ys.extend([o.y, o.y + o.h / 2.0, o.bottom()]);
    }
    let best = |mine: [f32; 3], targets: &[f32]| -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        for m in mine {
            for t in targets {
                let d = t - m;
                if d.abs() <= threshold && best.map(|(b, _)| d.abs() < b.abs()).unwrap_or(true) {
                    best = Some((d, *t));
                }
            }
        }
        best
    };
    let bx = best([moving.x, moving.x + moving.w / 2.0, moving.right()], &xs);
    let by = best([moving.y, moving.y + moving.h / 2.0, moving.bottom()], &ys);
    let mut guides = Vec::new();
    if let Some((_, g)) = bx {
        guides.push(Guide::Vertical(g));
    }
    if let Some((_, g)) = by {
        guides.push(Guide::Horizontal(g));
    }
    ((bx.map(|b| b.0).unwrap_or(0.0), by.map(|b| b.0).unwrap_or(0.0)), guides)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui(tree: &mut Tree, class: &str, parent: Id, name: &str, props: Vec<(&str, Value)>) -> Id {
        let id = tree.create(class);
        tree.set_parent(id, Some(parent)).unwrap();
        tree.set_prop(id, "Name", Value::String(name.into())).unwrap();
        for (k, v) in props {
            tree.set_prop(id, k, v).unwrap();
        }
        id
    }
    fn u2(sx: f32, ox: i32, sy: f32, oy: i32) -> Value {
        Value::UDim2 { sx, ox, sy, oy }
    }
    fn root() -> (Tree, Id) {
        let mut t = Tree::new();
        let sg = gui(&mut t, "ScreenGui", Tree::ROOT, "Gui", vec![]);
        (t, sg)
    }
    fn rect_of(nodes: &[GuiNode], id: Id) -> Rect {
        nodes.iter().find(|n| n.id == id).unwrap().rect
    }

    #[test]
    fn scale_offset_and_anchor() {
        let (mut t, sg) = root();
        let f = gui(&mut t, "Frame", sg, "F", vec![("Size", u2(0.5, 10, 0.0, 100)), ("Position", u2(0.5, 0, 0.5, 0)), ("AnchorPoint", Value::Vector2([0.5, 0.5]))]);
        let n = layout(&t, sg, (800.0, 400.0));
        assert_eq!(rect_of(&n, f), Rect::new(400.0 - 205.0, 200.0 - 50.0, 410.0, 100.0));
        let child = gui(&mut t, "Frame", f, "C", vec![("Size", u2(1.0, 0, 0.5, 0)), ("Position", u2(0.0, 5, 0.0, 5))]);
        let n = layout(&t, sg, (800.0, 400.0));
        assert_eq!(rect_of(&n, child), Rect::new(195.0 + 5.0, 150.0 + 5.0, 410.0, 50.0));
        let order: Vec<Id> = n.iter().map(|x| x.id).collect();
        assert!(order.iter().position(|i| *i == f) < order.iter().position(|i| *i == child), "parent paints before child");
    }

    #[test]
    fn visibility_zindex_clipping_and_hit_test() {
        let (mut t, sg) = root();
        let a = gui(&mut t, "Frame", sg, "A", vec![("Size", u2(0.0, 100, 0.0, 100)), ("ZIndex", Value::Int(2)), ("ClipsDescendants", Value::Bool(true))]);
        let b = gui(&mut t, "Frame", sg, "B", vec![("Size", u2(0.0, 100, 0.0, 100)), ("ZIndex", Value::Int(1))]);
        let hidden = gui(&mut t, "Frame", sg, "H", vec![("Size", u2(0.0, 300, 0.0, 300)), ("Visible", Value::Bool(false))]);
        let big = gui(&mut t, "Frame", a, "Big", vec![("Size", u2(0.0, 500, 0.0, 500))]);
        let n = layout(&t, sg, (800.0, 600.0));
        assert!(n.iter().all(|x| x.id != hidden));
        assert_eq!(hit_test(&n, 50.0, 50.0), Some(big), "child of the higher ZIndex sibling is on top");
        assert_eq!(hit_test(&n, 300.0, 300.0), None, "clipped outside the clipping parent");
        let _ = b;
    }

    #[test]
    fn paint_order_is_depth_first_by_zindex() {
        let (mut t, sg) = root();
        let b = gui(&mut t, "Frame", sg, "B", vec![("ZIndex", Value::Int(2))]);
        let a = gui(&mut t, "Frame", sg, "A", vec![("ZIndex", Value::Int(1))]);
        let a1 = gui(&mut t, "Frame", a, "A1", vec![]);
        let b1 = gui(&mut t, "Frame", b, "B1", vec![]);
        let order: Vec<Id> = layout(&t, sg, (100.0, 100.0)).iter().map(|n| n.id).collect();
        assert_eq!(order, vec![a, a1, b, b1]);
    }

    #[test]
    fn list_layout_padding_and_alignment() {
        let (mut t, sg) = root();
        let f = gui(&mut t, "Frame", sg, "F", vec![("Size", u2(0.0, 200, 0.0, 300))]);
        gui(&mut t, "UIListLayout", f, "L", vec![("Padding", Value::UDim(0.0, 10)), ("SortOrder", Value::Enum(2)), ("HorizontalAlignment", Value::Enum(1))]);
        gui(&mut t, "UIPadding", f, "P", vec![("PaddingTop", Value::UDim(0.0, 20)), ("PaddingLeft", Value::UDim(0.0, 0))]);
        let b = gui(&mut t, "Frame", f, "B", vec![("Size", u2(0.0, 100, 0.0, 50)), ("LayoutOrder", Value::Int(2))]);
        let a = gui(&mut t, "Frame", f, "A", vec![("Size", u2(0.0, 100, 0.0, 50)), ("LayoutOrder", Value::Int(1))]);
        let n = layout(&t, sg, (800.0, 600.0));
        assert_eq!(rect_of(&n, a), Rect::new(50.0, 20.0, 100.0, 50.0));
        assert_eq!(rect_of(&n, b), Rect::new(50.0, 80.0, 100.0, 50.0));
    }

    #[test]
    fn grid_aspect_and_size_constraint() {
        let (mut t, sg) = root();
        let f = gui(&mut t, "Frame", sg, "F", vec![("Size", u2(0.0, 220, 0.0, 400))]);
        gui(&mut t, "UIGridLayout", f, "G", vec![("CellSize", u2(0.0, 100, 0.0, 50)), ("CellPadding", u2(0.0, 10, 0.0, 10)), ("SortOrder", Value::Enum(2))]);
        let ids: Vec<Id> = (0..3).map(|i| gui(&mut t, "Frame", f, &format!("C{i}"), vec![("LayoutOrder", Value::Int(i))])).collect();
        let n = layout(&t, sg, (800.0, 600.0));
        assert_eq!(rect_of(&n, ids[0]), Rect::new(0.0, 0.0, 100.0, 50.0));
        assert_eq!(rect_of(&n, ids[1]), Rect::new(110.0, 0.0, 100.0, 50.0));
        assert_eq!(rect_of(&n, ids[2]), Rect::new(0.0, 60.0, 100.0, 50.0));

        let (mut t, sg) = root();
        let f = gui(&mut t, "Frame", sg, "F", vec![("Size", u2(0.0, 400, 0.0, 100))]);
        gui(&mut t, "UIAspectRatioConstraint", f, "A", vec![("AspectRatio", Value::Float(1.0))]);
        let n = layout(&t, sg, (800.0, 600.0));
        assert_eq!(rect_of(&n, f).w, 100.0);
        gui(&mut t, "UISizeConstraint", f, "S", vec![("MaxSize", Value::Vector2([50.0, 50.0]))]);
        assert_eq!(rect_of(&layout(&t, sg, (800.0, 600.0)), f).w, 50.0);
    }

    #[test]
    fn disabled_screengui_and_missing_root() {
        let (mut t, sg) = root();
        gui(&mut t, "Frame", sg, "F", vec![]);
        t.set_prop(sg, "Enabled", Value::Bool(false)).unwrap();
        assert!(layout(&t, sg, (100.0, 100.0)).is_empty());
        assert!(layout(&t, 9999, (100.0, 100.0)).is_empty());
    }

    #[test]
    fn move_and_resize_math() {
        let p = (0.5, 10.0, 0.0, 20.0);
        assert_eq!(move_udim2(p, 5.0, -4.0, (800.0, 400.0), Mode::Offset), (0.5, 15.0, 0.0, 16.0));
        let m = move_udim2(p, 80.0, 40.0, (800.0, 400.0), Mode::Scale);
        assert!((m.0 - 0.6).abs() < 1e-6 && m.1 == 10.0 && (m.2 - 0.1).abs() < 1e-6 && m.3 == 20.0);

        // Drag the right edge of a frame anchored at the center: width grows, position shifts by half.
        let (np, ns) = resize_udim2((0.0, 100.0, 0.0, 100.0), (0.0, 50.0, 0.0, 50.0), [0.5, 0.5], (1, 0), 20.0, 0.0, (800.0, 600.0), Mode::Offset, (50.0, 50.0), 4.0);
        assert_eq!(ns, (0.0, 70.0, 0.0, 50.0));
        assert_eq!(np, (0.0, 110.0, 0.0, 100.0));
        // Drag the left edge of a top-left anchored frame: position follows the edge, width shrinks.
        let (np, ns) = resize_udim2((0.0, 100.0, 0.0, 100.0), (0.0, 50.0, 0.0, 50.0), [0.0, 0.0], (-1, 0), 10.0, 0.0, (800.0, 600.0), Mode::Offset, (50.0, 50.0), 4.0);
        assert_eq!((np.1, ns.1), (110.0, 40.0));
        // Minimum size clamps.
        let (_, ns) = resize_udim2((0.0, 0.0, 0.0, 0.0), (0.0, 50.0, 0.0, 50.0), [0.0, 0.0], (1, 1), -500.0, -500.0, (800.0, 600.0), Mode::Offset, (50.0, 50.0), 4.0);
        assert_eq!((ns.1, ns.3), (4.0, 4.0));
    }

    #[test]
    fn snapping() {
        let parent = Rect::new(0.0, 0.0, 800.0, 600.0);
        let others = [Rect::new(200.0, 100.0, 100.0, 100.0)];
        let ((dx, dy), g) = snap_to_guides(Rect::new(303.0, 98.0, 50.0, 50.0), &others, parent, 6.0);
        assert_eq!((dx, dy), (-3.0, 2.0));
        assert_eq!(g.len(), 2);
        let ((dx, dy), g) = snap_to_guides(Rect::new(500.0, 400.0, 50.0, 50.0), &others, parent, 6.0);
        assert_eq!((dx, dy), (0.0, 0.0));
        assert!(g.is_empty());
        let ((dx, _), _) = snap_to_guides(Rect::new(375.0, 400.0, 50.0, 50.0), &others, parent, 6.0);
        assert_eq!(dx, 0.0, "centered on the parent already");
    }
}
