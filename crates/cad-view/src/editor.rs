//! Editing in the viewer: the command line, the drawing tools' clicks,
//! selection, undo and redo, the layer list, saving and new drawings. All of
//! it goes through the session's text commands, as the browser app's does,
//! and the display is read again after each edit.

use crate::display::{Bounds, Display, Item, Rgb, Style};
use crate::draw::{Prim, tessellate, tessellate_item};
use crate::json::Json;
use crate::query::parse_handle;
use crate::tool::{self, Tool};
use crate::viewer::Viewer;

/// Lines of command history kept.
pub const MAX_LOG: usize = 200;

/// One layer as the core lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerInfo {
    pub handle: u64,
    pub name: String,
    pub color: Rgb,
    pub on: bool,
    pub frozen: bool,
    pub locked: bool,
    pub current: bool,
    pub count: u32,
}

/// Commands that change nothing, so the display need not be read again.
const QUERIES: [&str; 14] = [
    "get",
    "snap",
    "pick",
    "box",
    "len",
    "angle",
    "fmt",
    "layouts",
    "blocks",
    "linetypes",
    "patterns",
    "attribs",
    "bounds",
    "measure",
];

/// What a short word on the command line stands for when it has arguments.
fn expand(word: &str) -> &str {
    match word {
        "l" => "line",
        "c" | "ci" => "circle",
        "a" => "arc-3p",
        "arc" => "arc-3p",
        "pl" | "polyline" => "pline",
        "rec" | "rectangle" => "rect",
        "e" | "delete" | "del" => "erase",
        "u" => "undo",
        other => other,
    }
}

impl Viewer {
    pub fn log_line(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.message = s.clone();
        self.log.push(s);
        if self.log.len() > MAX_LOG {
            self.log.drain(..self.log.len() - MAX_LOG);
        }
    }

    /// Reads what the core says about the drawing: units, edit counts, layers.
    pub(crate) fn refresh_info(&mut self) {
        let Some(id) = self.id else { return };
        let Some(j) = self
            .session
            .info(id)
            .ok()
            .and_then(|t| Json::parse(&t).ok())
        else {
            return;
        };
        self.units = j
            .get("units")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        self.dirty = matches!(j.get("dirty"), Some(Json::Bool(true)));
        let n = |k: &str| {
            j.get(k)
                .and_then(Json::as_f64)
                .filter(|v| *v >= 0.0)
                .map_or(0, |v| v as usize)
        };
        self.undo_depth = n("undo");
        self.redo_depth = n("redo");
        self.layer_infos = j
            .get("layers")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(|l| {
                let flag = |k: &str| matches!(l.get(k), Some(Json::Bool(true)));
                Some(LayerInfo {
                    handle: l
                        .get("handle")
                        .and_then(Json::as_str)
                        .and_then(parse_handle)?,
                    name: l.get("name").and_then(Json::as_str)?.to_string(),
                    color: l
                        .get("color")
                        .and_then(Json::as_str)
                        .and_then(Rgb::from_hex)
                        .unwrap_or(Rgb(200, 200, 200)),
                    on: flag("on"),
                    frozen: flag("frozen"),
                    locked: flag("locked"),
                    current: flag("current"),
                    count: l
                        .get("count")
                        .and_then(Json::as_f64)
                        .map_or(0, |c| c.max(0.0) as u32),
                })
            })
            .collect();
    }

    /// Reads the display and the info again, keeping the view; selected
    /// objects that are gone are dropped from the selection.
    pub fn refresh(&mut self) {
        let Some(id) = self.id else { return };
        match self.session.display(id).and_then(|b| Display::parse(&b)) {
            Ok(d) => self.display = d,
            Err(e) => self.log_line(format!("display: {e}")),
        }
        self.refresh_info();
        let before = self.selected.len();
        let display = &self.display;
        self.selected
            .retain(|h| display.items_of(*h).next().is_some());
        if self.selected.len() != before {
            self.update_selection_text();
        }
        self.hover = None;
    }

    /// One command to the core. The display is read again after an edit.
    /// Returns the handles the edit made or changed.
    pub fn execute(&mut self, cmd: &str) -> Result<Vec<u64>, String> {
        let id = self.id.ok_or("no drawing is open")?;
        let reply = self.session.exec(id, cmd)?;
        let first = cmd
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if !QUERIES.contains(&first.as_str()) {
            self.refresh();
        }
        Ok(Json::parse(&reply)
            .ok()
            .and_then(|j| {
                j.get("handles").and_then(Json::as_array).map(|a| {
                    a.iter()
                        .filter_map(|h| h.as_str().and_then(parse_handle))
                        .collect()
                })
            })
            .unwrap_or_default())
    }

    /// A command from a tool or a button: logged, and its outcome said.
    fn run_command(&mut self, cmd: &str) -> Option<Vec<u64>> {
        self.log.push(format!("> {cmd}"));
        match self.execute(cmd) {
            Ok(h) => {
                let word = cmd.split_whitespace().next().unwrap_or("");
                self.log_line(match h.len() {
                    0 => format!("{word}: done"),
                    1 => format!("{word}: 1 object"),
                    n => format!("{word}: {n} objects"),
                });
                Some(h)
            }
            Err(e) => {
                self.log_line(e);
                None
            }
        }
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.points.clear();
        self.message = t.prompt(0).to_string();
    }

    /// What the tool waits for.
    pub fn prompt(&self) -> String {
        self.tool.prompt(self.points.len()).to_string()
    }

    /// A click on the canvas at the screen point `at`: a point for the
    /// tool, or the object under it. `add` adds to the selection.
    pub fn canvas_click(&mut self, at: [f32; 2], add: bool) {
        self.hover_at(Some(at));
        let Some(h) = self.hover.clone() else { return };
        let p = h.snap.as_ref().map_or(h.world, |s| s.point);
        match self.tool {
            Tool::Select => match h.handle {
                Some(e) if add => self.toggle_selected(e),
                Some(e) => self.select(&[e]),
                None if !add => self.select(&[]),
                None => {}
            },
            Tool::Erase => {
                if let Some(e) = h.handle {
                    self.run_command(&format!("erase {e:X}"));
                }
            }
            t => self.point(t, p),
        }
    }

    /// A point for the drawing tool; runs the command it completes.
    pub fn point(&mut self, t: Tool, p: [f64; 2]) {
        if let Some(cmd) = tool::click(t, &mut self.points, p) {
            self.run_command(&cmd);
        }
        self.message = self.prompt();
    }

    /// Ends the tool's object: a polyline, open or closed.
    pub fn finish(&mut self, closed: bool) {
        if self.tool == Tool::Polyline {
            let pts = std::mem::take(&mut self.points);
            match tool::polyline(&pts, closed) {
                Some(cmd) => {
                    self.run_command(&cmd);
                }
                None if !pts.is_empty() => self.log_line("a polyline needs two points"),
                None => {}
            }
        } else {
            self.points.clear();
        }
        self.message = self.prompt();
    }

    /// Esc: drop the points; with none, leave the tool; with none of that,
    /// clear the selection.
    pub fn cancel(&mut self) {
        if !self.points.is_empty() {
            self.points.clear();
        } else if self.tool != Tool::Select {
            self.tool = Tool::Select;
        } else {
            self.select(&[]);
        }
        self.message = self.prompt();
    }

    pub fn select(&mut self, handles: &[u64]) {
        self.selected = handles.to_vec();
        self.update_selection_text();
    }

    fn toggle_selected(&mut self, h: u64) {
        match self.selected.iter().position(|x| *x == h) {
            Some(i) => {
                self.selected.remove(i);
            }
            None => self.selected.push(h),
        }
        self.update_selection_text();
    }

    fn update_selection_text(&mut self) {
        self.selection_text = match self.selected.as_slice() {
            [] => String::new(),
            [h] => {
                let cmd = format!("get {h:X}");
                match self.id.map(|id| self.session.exec(id, &cmd)) {
                    Some(Ok(t)) => describe(&t),
                    Some(Err(e)) => e,
                    None => String::new(),
                }
            }
            many => format!("{} objects selected", many.len()),
        };
    }

    pub fn erase_selected(&mut self) {
        if self.selected.is_empty() {
            self.log_line("nothing is selected");
            return;
        }
        let list: Vec<String> = self.selected.iter().map(|h| format!("{h:X}")).collect();
        self.run_command(&format!("erase {}", list.join(" ")));
    }

    pub fn undo(&mut self) {
        self.run_command("undo");
    }

    pub fn redo(&mut self) {
        self.run_command("redo");
    }

    pub fn set_layer_on(&mut self, handle: u64, on: bool) {
        self.run_command(&format!("layer-on {handle:X} {}", u8::from(on)));
    }

    pub fn set_layer_current(&mut self, handle: u64) {
        self.run_command(&format!("layer-current {handle:X}"));
    }

    /// One line typed on the command line, or its Enter when empty.
    pub fn run(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            // Enter ends a polyline, as in the CAD programs people know.
            if self.tool == Tool::Polyline {
                self.finish(false);
            }
            return;
        }
        let spaced = line.replace(',', " ");
        let words: Vec<&str> = spaced.split_whitespace().collect();
        let Some((&first, rest)) = words.split_first() else {
            return;
        };
        let word = first.to_ascii_lowercase();

        // Coordinates answer the tool's prompt: `10,20`, `@5,0` from the
        // last point, or a radius.
        if self.tool.takes_points() {
            if let Some(p) = self.typed_point(&words) {
                self.log.push(format!("> {line}"));
                self.point(self.tool, p);
                return;
            }
            // Written without `let` chains: they need Rust 1.88 and the
            // workspace builds with 1.87.
            let circle = match (self.tool, self.points.as_slice(), words.as_slice()) {
                (Tool::Circle, [c], [r]) => r.parse::<f64>().ok().and_then(|r| tool::circle(*c, r)),
                _ => None,
            };
            if let Some(cmd) = circle {
                self.points.clear();
                self.log.push(format!("> {line}"));
                self.run_command(&cmd);
                return;
            }
        }

        if rest.is_empty() {
            match word.as_str() {
                "fit" | "z" | "ze" | "zoom" | "extents" => {
                    self.zoom_extents();
                    self.log_line("zoom: extents");
                    return;
                }
                "grid" => {
                    self.show_grid = !self.show_grid;
                    self.log_line(format!(
                        "grid {}",
                        if self.show_grid { "on" } else { "off" }
                    ));
                    return;
                }
                "paper" => {
                    self.options.paper = !self.options.paper;
                    return;
                }
                "snap" => {
                    self.snap_on = !self.snap_on;
                    self.log_line(format!("snap {}", if self.snap_on { "on" } else { "off" }));
                    return;
                }
                "u" | "undo" => return self.undo(),
                "redo" => return self.redo(),
                "close" | "cl" if self.tool == Tool::Polyline => return self.finish(true),
                "help" | "?" => {
                    self.log.push("> help".into());
                    for l in HELP {
                        self.log.push((*l).into());
                    }
                    self.message = HELP[0].into();
                    return;
                }
                "e" | "erase" | "delete" | "del" if !self.selected.is_empty() => {
                    return self.erase_selected();
                }
                w => {
                    if let Some(t) = Tool::from_word(w) {
                        self.log.push(format!("> {line}"));
                        self.set_tool(t);
                        return;
                    }
                }
            }
        }
        if (word == "zoom" || word == "z")
            && matches!(rest.first(), Some(&"e" | &"extents" | &"all" | &"a"))
        {
            self.zoom_extents();
            return;
        }
        let cmd = format!("{} {}", expand(&word), rest.join(" "));
        self.run_command(cmd.trim());
    }

    /// `x y`, or `@dx dy` from the last point.
    fn typed_point(&self, words: &[&str]) -> Option<[f64; 2]> {
        let [a, b] = words else { return None };
        if let Some(a) = a.strip_prefix('@') {
            let last = self.points.last()?;
            return Some([
                last[0] + a.parse::<f64>().ok()?,
                last[1] + b.parse::<f64>().ok()?,
            ]);
        }
        Some([a.parse().ok()?, b.parse().ok()?])
    }

    /// The drawing in a file format (`cad`, `dwg`, `dxf`, `dxfb`, `svg`,
    /// `pdf`) and the notes about what it could not hold.
    pub fn save_bytes(&mut self, format: &str) -> Result<(Vec<u8>, Vec<String>), String> {
        let id = self.id.ok_or("no drawing is open")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
        let r = self.session.save(id, format, now, "BlackCAD");
        self.refresh_info();
        r
    }

    /// A new, empty drawing from the metric or the imperial template.
    pub fn new_drawing(&mut self, imperial: bool) -> Result<(), String> {
        let id = self.session.new_drawing(imperial)?;
        self.adopt(
            id,
            if imperial {
                "Drawing (inches)"
            } else {
                "Drawing (mm)"
            },
        )?;
        // An empty drawing has no extents: show a sheet-sized patch of it.
        let (w, h) = if imperial {
            (12.0, 8.0)
        } else {
            (300.0, 200.0)
        };
        self.view.zoom_extents(&Bounds::new(0.0, 0.0, w, h), 24.0);
        Ok(())
    }

    /// `name`, marked when there are unsaved edits.
    pub fn title(&self) -> String {
        format!("{}{}", self.name, if self.dirty { " *" } else { "" })
    }

    /// The rubber band for the tool at the pointer, as screen shapes in
    /// one style (item 0).
    pub fn preview_prims(&self) -> Vec<Prim> {
        let Some(h) = &self.hover else {
            return Vec::new();
        };
        let to = h.snap.as_ref().map_or(h.world, |s| s.point);
        let ops = tool::preview(self.tool, &self.points, to);
        if ops.is_empty() {
            return Vec::new();
        }
        let mut bounds = Bounds::EMPTY;
        ops.iter().for_each(|o| bounds.union(&o.bounds()));
        let style = Style {
            screen: Rgb(255, 255, 255),
            print: Rgb(0, 0, 0),
            px: 1.0,
            layer: String::new(),
            linetype: String::new(),
        };
        let d = Display {
            bounds: None,
            styles: vec![style],
            items: vec![Item {
                handle: 0,
                style: 0,
                ops,
                bounds,
            }],
            skipped: Vec::new(),
        };
        tessellate(&d, &self.view, &crate::draw::DrawOptions::default())
    }

    /// The selected objects as screen shapes, to draw them again lit.
    pub fn selection_prims(&self) -> Vec<Prim> {
        let mut out = Vec::new();
        for h in &self.selected {
            for i in self.display.items_of(*h) {
                tessellate_item(&self.display, i, &self.view, &self.options, &mut out);
            }
        }
        out
    }
}

/// What the command line's `help` says.
pub const HELP: [&str; 9] = [
    "Commands: type a word and press Enter; typing on the canvas goes here too.",
    "  line, circle, rect, pline, arc, erase: start the tool, then click points or type x,y (@dx,dy from the last point)",
    "  line 0 0 100 50 | circle 50 50 20 | rect 0 0 100 60: draw at once",
    "  move dx dy h | copy dx dy h | rotate cx cy deg h | mirror ... | offset ... | trim ... | fillet ...: edit (COMMANDS.md)",
    "  undo, redo (Ctrl+Z, Ctrl+Y); erase with objects selected deletes them",
    "  zoom e (or Home): fit the drawing; grid, snap, paper: switch them",
    "  layer-add name | layer-on h 0/1 | layer-current h",
    "Mouse: wheel zooms, drag pans, click selects or places a point, right-click ends a polyline.",
    "Esc: cancel the tool, then clear the selection.",
];

/// A reply of the core's `get` as lines of `name: value`.
pub fn describe(reply: &str) -> String {
    let Ok(Json::Obj(fields)) = Json::parse(reply) else {
        return reply.to_string();
    };
    let mut out = Vec::new();
    for (k, v) in &fields {
        let text = match v {
            Json::Str(s) => s.clone(),
            Json::Num(n) => format!("{n}"),
            Json::Bool(b) => b.to_string(),
            Json::Null => continue,
            Json::Arr(a) => format!("{} values", a.len()),
            Json::Obj(o) => format!("{} fields", o.len()),
        };
        out.push(format!("{k}: {text}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Viewer {
        let mut v = Viewer::new();
        v.set_size([800.0, 600.0]);
        v.new_drawing(false).unwrap();
        v
    }

    fn items(v: &Viewer) -> usize {
        v.display.items.len()
    }

    #[test]
    fn a_new_drawing_is_empty_and_in_view() {
        let v = blank();
        assert!(v.is_open() && items(&v) == 0);
        assert!(v.title().starts_with("Drawing (mm)") && !v.dirty);
        assert!(!v.layer_infos.is_empty(), "layer 0 at least");
        assert!(v.layer_infos.iter().any(|l| l.current));
        let wb = v.view.world_bounds();
        assert!(wb.min[0] <= 0.0 && wb.max[0] >= 300.0 && wb.min[1] <= 0.0 && wb.max[1] >= 200.0);
        let imp = {
            let mut v = Viewer::new();
            v.set_size([800.0, 600.0]);
            v.new_drawing(true).unwrap();
            v
        };
        assert!(imp.view.world_bounds().max[0] >= 12.0 && imp.view.world_bounds().max[0] < 100.0);
    }

    #[test]
    fn commands_draw_and_undo_redo_walk_the_history() {
        let mut v = blank();
        v.run("line 0 0 100 50");
        assert_eq!(items(&v), 1);
        assert!(v.dirty && v.undo_depth == 1, "{}", v.undo_depth);
        v.run("circle 50 50 20");
        v.run("rect 10 10 60 40");
        assert_eq!(items(&v), 3);
        v.run("undo");
        assert_eq!(items(&v), 2);
        v.run("u");
        assert_eq!(items(&v), 1);
        assert_eq!(v.redo_depth, 2);
        v.run("redo");
        assert_eq!(items(&v), 2);
        v.undo();
        v.undo();
        v.undo();
        assert!(v.message.contains("nothing to undo"), "{}", v.message);
        assert_eq!(items(&v), 0);
    }

    #[test]
    fn commas_and_short_words_work_with_arguments() {
        let mut v = blank();
        v.run("l 0,0 100,0");
        v.run("c 50,50 25");
        v.run("pl 0 0,0 0 100,0 0 100,50,0");
        assert_eq!(items(&v), 3, "{:?}", v.log);
        v.run("rec 0,0 20,20");
        assert_eq!(items(&v), 4);
        v.run("a 0,0 5,5 10,0");
        assert_eq!(items(&v), 5, "{:?}", v.log);
    }

    #[test]
    fn a_bad_command_says_so_and_changes_nothing() {
        let mut v = blank();
        v.run("circle 1 2");
        assert_eq!(items(&v), 0);
        assert!(
            !v.message.is_empty() && v.log.iter().any(|l| l == "> circle 1 2"),
            "{:?}",
            v.log
        );
        v.run("frobnicate 1 2 3");
        assert!(!v.dirty);
        v.run("rect 5 5 5 9");
        assert_eq!(items(&v), 0);
    }

    #[test]
    fn tools_take_typed_points_and_clicks() {
        let mut v = blank();
        v.run("line");
        assert_eq!(v.tool, Tool::Line);
        v.run("0,0");
        v.run("100,0");
        v.run("@0,50");
        assert_eq!(items(&v), 2, "a chain of two segments: {:?}", v.log);
        v.cancel();
        assert!(v.points.is_empty() && v.tool == Tool::Line);
        v.cancel();
        assert_eq!(v.tool, Tool::Select);

        v.run("circle");
        v.run("50,50");
        v.run("12.5");
        assert_eq!(items(&v), 3, "{:?}", v.log);

        v.run("pline");
        v.run("0,0");
        v.run("30,0");
        v.run("30,30");
        v.run("");
        assert_eq!(items(&v), 4, "{:?}", v.log);
        v.run("pline");
        v.run("0,0");
        v.run("30,0");
        v.run("30,30");
        v.run("close");
        assert_eq!(items(&v), 5);
        assert!(v.points.is_empty());
    }

    #[test]
    fn clicks_on_the_canvas_draw_with_the_pointer() {
        let mut v = blank();
        v.set_tool(Tool::Line);
        let a = v.view.to_screen([20.0, 20.0]);
        let b = v.view.to_screen([120.0, 70.0]);
        v.canvas_click(a, false);
        assert_eq!(v.points.len(), 1);
        v.hover_at(Some(b));
        assert_eq!(v.preview_prims().len(), 1, "the rubber band");
        v.canvas_click(b, false);
        assert_eq!(items(&v), 1);
        // the line goes on from its end
        v.canvas_click(v.view.to_screen([200.0, 100.0]), false);
        assert_eq!(items(&v), 2, "{:?}", v.log);
        // a click near an end of the first line takes that end exactly
        v.cancel();
        v.set_tool(Tool::Line);
        let Some(crate::display::Op::Line(first)) = v.display.items[0].ops.first().cloned() else {
            panic!("a line")
        };
        v.canvas_click(v.view.to_screen([first[0] + 0.3, first[1] - 0.2]), false);
        assert_eq!(v.points, vec![[first[0], first[1]]], "snapped to the end");
    }

    #[test]
    fn select_shows_what_the_core_says_and_erase_removes_it() {
        let mut v = blank();
        v.run("line 0 0 100 0");
        v.run("line 0 50 100 50");
        v.set_tool(Tool::Select);
        let on_first = v.view.to_screen([50.0, 0.0]);
        v.canvas_click(on_first, false);
        assert_eq!(v.selected.len(), 1);
        assert!(
            v.selection_text.contains("line") || v.selection_text.contains("type"),
            "{}",
            v.selection_text
        );
        assert!(!v.selection_prims().is_empty());
        let on_second = v.view.to_screen([50.0, 50.0]);
        v.canvas_click(on_second, true);
        assert_eq!(v.selected.len(), 2);
        assert!(v.selection_text.contains("2 objects"));
        v.canvas_click(on_second, true);
        assert_eq!(v.selected.len(), 1, "shift-click toggles");
        v.run("erase");
        assert_eq!(items(&v), 1);
        assert!(v.selected.is_empty());
        // clicking nothing clears the selection
        v.canvas_click(on_second, false);
        v.canvas_click(v.view.to_screen([200.0, 150.0]), false);
        assert!(v.selected.is_empty());
        // undo brings the object back
        v.undo();
        assert_eq!(items(&v), 2);
    }

    #[test]
    fn the_erase_tool_erases_what_is_clicked() {
        let mut v = blank();
        v.run("circle 50 50 20");
        v.set_tool(Tool::Erase);
        v.canvas_click(v.view.to_screen([200.0, 150.0]), false);
        assert_eq!(items(&v), 1, "an empty click erases nothing");
        v.canvas_click(v.view.to_screen([70.0, 50.0]), false);
        assert_eq!(items(&v), 0);
    }

    #[test]
    fn layers_turn_off_and_on_through_the_core() {
        let mut v = blank();
        v.run("layer-add Walls");
        let walls = v
            .layer_infos
            .iter()
            .find(|l| l.name == "Walls")
            .expect("added")
            .clone();
        assert!(walls.on);
        v.set_layer_current(walls.handle);
        assert!(
            v.layer_infos
                .iter()
                .find(|l| l.name == "Walls")
                .unwrap()
                .current
        );
        v.run("line 0 0 10 10");
        assert_eq!(items(&v), 1);
        v.set_layer_on(walls.handle, false);
        assert_eq!(items(&v), 0, "a layer that is off draws nothing");
        assert!(!v.layer_infos.iter().find(|l| l.name == "Walls").unwrap().on);
        v.set_layer_on(walls.handle, true);
        assert_eq!(items(&v), 1);
    }

    #[test]
    fn view_words_and_help_need_no_drawing_command() {
        let mut v = blank();
        v.run("line 0 0 1000 0");
        v.run("zoom e");
        let fitted = v.view;
        v.view.zoom_about(10.0, [400.0, 300.0]);
        v.run("fit");
        assert_eq!(v.view, fitted);
        assert!(v.show_grid);
        v.run("grid");
        assert!(!v.show_grid);
        v.run("snap");
        assert!(!v.snap_on);
        v.run("paper");
        assert!(v.options.paper);
        let n = v.log.len();
        v.run("help");
        assert!(v.log.len() > n + 3);
        assert!(
            !v.dirty || v.undo_depth == 1,
            "words that are not commands change nothing"
        );
    }

    #[test]
    fn saves_in_every_format_and_clears_the_dirty_mark() {
        let mut v = blank();
        v.run("line 0 0 100 50");
        v.run("circle 50 50 20");
        assert!(v.dirty && v.title().ends_with(" *"));
        for f in ["dxf", "dwg", "cad"] {
            let (bytes, _) = v.save_bytes(f).unwrap();
            let mut w = Viewer::new();
            w.open(&format!("saved.{f}"), &bytes).unwrap();
            assert_eq!(w.display.items.len(), 2, "{f}");
        }
        assert!(!v.dirty, "saved");
        assert!(v.save_bytes("nonsense").is_err());
        assert!(Viewer::new().save_bytes("dxf").is_err());
    }

    #[test]
    fn nothing_open_is_an_error_not_a_panic() {
        let mut v = Viewer::new();
        v.run("line 0 0 1 1");
        assert!(v.message.contains("no drawing"), "{}", v.message);
        v.canvas_click([5.0, 5.0], false);
        v.erase_selected();
        v.undo();
        v.finish(false);
        v.cancel();
        assert!(v.preview_prims().is_empty() && v.selection_prims().is_empty());
    }

    #[test]
    fn the_log_is_bounded() {
        let mut v = blank();
        for i in 0..MAX_LOG + 50 {
            v.log_line(format!("line {i}"));
        }
        assert_eq!(v.log.len(), MAX_LOG);
        assert_eq!(v.log.last().unwrap(), &format!("line {}", MAX_LOG + 49));
    }

    #[test]
    fn describe_lists_fields() {
        let t =
            describe(r#"{"type":"line","layer":"0","x1":1.5,"gone":null,"pts":[1,2],"open":true}"#);
        assert!(
            t.contains("type: line")
                && t.contains("x1: 1.5")
                && t.contains("pts: 2 values")
                && t.contains("open: true")
        );
        assert!(!t.contains("gone"));
        assert_eq!(describe("not json"), "not json");
    }
}
