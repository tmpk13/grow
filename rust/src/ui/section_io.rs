//! Taking one section of the menu away with you.
//!
//! Every settings row a panel draws goes through `ui::row`, which stamps it
//! with the slug menu search jumps to and puts the name in a known span. That
//! is enough to read a section back out of the page it drew itself into, so
//! this needs no schema of its own and no panel has to be told about it: a
//! section written today can be copied today.
//!
//! What comes out is the section as it stands, not a patch and not a project.
//! Copy puts it on the clipboard, Save writes the same text to a file, and a
//! section with no settings in it - a roster, a graph, a list of the dead -
//! gets neither, because there would be nothing in the file.

use wasm_bindgen::{JsCast, JsValue};
use web_sys::Element;

use crate::ui::{document, el, window, Scope};
use crate::util::{file_name, slug};

/// The pair of buttons for a section head, or nothing when the section holds
/// no settings.
pub fn tools(title: &str, body: &Element) -> Option<Element> {
    if fields(body).is_empty() {
        return None;
    }

    let copied = {
        let (title, body) = (title.to_string(), body.clone());
        move || {
            copy_text(&gather(&title, &body));
        }
    };
    let saved = {
        let (title, body) = (title.to_string(), body.clone());
        move || {
            let text = gather(&title, &body);
            crate::ui::save_bytes(
                text.as_bytes(),
                "application/json",
                &file_name(&format!("grow-{}", slug(&title)), "json"),
            );
        }
    };

    Some(
        el("span")
            .class("group-tools")
            .child(&tool("Copy", "Copy these settings as text", copied))
            .child(&tool("Save", "Save these settings to a file", saved))
            .get(),
    )
}

/// One of them. A press inside a summary would fold the section it is the head
/// of, so the button has to say that this press was not about the fold.
fn tool(text: &str, title: &str, mut act: impl FnMut() + 'static) -> Element {
    el("button")
        .class("btn tiny")
        .attr("type", "button")
        .attr("title", title)
        .text(text)
        .on("click", Scope::Panel, move |e| {
            e.prevent_default();
            e.stop_propagation();
            act();
        })
        .get()
}

/// The section as JSON: what it is called, and every settings row in it with
/// the slug it is addressed by, the name it is shown under and what it is set
/// to right now.
fn gather(title: &str, body: &Element) -> String {
    let mut out = String::from("{\n  \"kind\": \"grow.menu-section\",\n  \"section\": ");
    out.push_str(&quote(title));
    out.push_str(",\n  \"fields\": [\n");
    let rows = fields(body);
    for (i, row) in rows.iter().enumerate() {
        let key = row.get_attribute("data-find").unwrap_or_default();
        let label = text_of(row, ".field-label");
        out.push_str("    { \"key\": ");
        out.push_str(&quote(&key));
        out.push_str(", \"label\": ");
        out.push_str(&quote(&label));
        out.push_str(", \"value\": ");
        out.push_str(&value_of(row));
        out.push_str(" }");
        if i + 1 < rows.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    out
}

/// The settings rows of a section, in the order they are shown. Only rows:
/// buttons carry a slug too, and a button has no value to write down.
fn fields(body: &Element) -> Vec<Element> {
    let mut out = Vec::new();
    let list = match body.query_selector_all("label.field[data-find]") {
        Ok(list) => list,
        Err(_) => return out,
    };
    for i in 0..list.length() {
        if let Some(node) = list.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
            out.push(node);
        }
    }
    out
}

/// What one row is set to, as a JSON value. Every control a row can hold reads
/// back from the page rather than from the model, which is what keeps this
/// from needing to know which panel drew it.
fn value_of(row: &Element) -> String {
    // A pair of numbers, low and high. Read before the single number, because
    // it holds number inputs too.
    if row.query_selector(".range-pair").ok().flatten().is_some() {
        let nums = numbers(row);
        if nums.len() == 2 {
            return format!("[{}, {}]", json_num(&nums[0]), json_num(&nums[1]));
        }
    }
    if let Some(input) = input(row, "input[type=\"number\"]") {
        return json_num(&input.value());
    }
    if let Some(input) = input(row, "input[type=\"color\"], input[type=\"text\"]") {
        return quote(&input.value());
    }
    if let Some(select) = row
        .query_selector("select")
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlSelectElement>().ok())
    {
        return quote(&select.value());
    }
    if let Some(switch) = row.query_selector("[aria-pressed]").ok().flatten() {
        return match crate::ui::pressed(&switch) {
            true => "true".into(),
            false => "false".into(),
        };
    }
    "null".into()
}

fn input(row: &Element, selector: &str) -> Option<web_sys::HtmlInputElement> {
    row.query_selector(selector)
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlInputElement>().ok())
}

fn numbers(row: &Element) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(list) = row.query_selector_all("input[type=\"number\"]") {
        for i in 0..list.length() {
            if let Some(n) = list.item(i).and_then(|n| n.dyn_into::<web_sys::HtmlInputElement>().ok())
            {
                out.push(n.value());
            }
        }
    }
    out
}

/// A number box that has been emptied holds no number, and `null` says that
/// more honestly than a zero somebody never typed.
fn json_num(raw: &str) -> String {
    match raw.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => {
            if (v - v.round()).abs() < 1e-9 {
                format!("{}", v.round() as i64)
            } else {
                let s = format!("{v:.6}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
        _ => "null".into(),
    }
}

fn text_of(row: &Element, selector: &str) -> String {
    row.query_selector(selector)
        .ok()
        .flatten()
        .and_then(|n| n.text_content())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Puts text on the clipboard.
///
/// Reached through reflection rather than through a typed binding: the
/// clipboard is still an unstable API in web-sys and turning it on is a build
/// flag on the whole crate. The older way is kept behind it because the
/// clipboard object only exists in a secure context, and a page served over
/// plain http on a home network is not one.
pub fn copy_text(text: &str) {
    let clipboard = js_sys::Reflect::get(&window(), &JsValue::from_str("navigator"))
        .and_then(|nav| js_sys::Reflect::get(&nav, &JsValue::from_str("clipboard")))
        .ok()
        .filter(|c| !c.is_undefined() && !c.is_null());
    if let Some(clipboard) = clipboard {
        if let Some(write) = method(&clipboard, "writeText") {
            // The promise is dropped rather than awaited: nothing here has
            // anything to do once the text is gone.
            if write.call1(&clipboard, &JsValue::from_str(text)).is_ok() {
                return;
            }
        }
    }
    copy_the_old_way(text);
}

/// A selection in a box nobody can see, and the command that copies a
/// selection. The box has to be in the page and on screen for the selection to
/// be real, so it is put there and taken away again in the same breath.
fn copy_the_old_way(text: &str) {
    let doc = document();
    let body = match doc.body() {
        Some(b) => b,
        None => return,
    };
    let area = match doc.create_element("textarea") {
        Ok(a) => a,
        Err(_) => return,
    };
    area.set_text_content(Some(text));
    if let Some(html) = area.dyn_ref::<web_sys::HtmlElement>() {
        let style = html.style();
        let _ = style.set_property("position", "fixed");
        let _ = style.set_property("top", "-1000px");
        let _ = style.set_property("opacity", "0");
    }
    let _ = body.append_child(&area);
    let _ = method(&area, "select").map(|f| f.call0(&area));
    let _ = method(&doc, "execCommand")
        .map(|f| f.call1(&doc, &JsValue::from_str("copy")));
    area.remove();
}

/// One named function off an object, for the calls web-sys has no binding for.
fn method(on: &JsValue, name: &str) -> Option<js_sys::Function> {
    js_sys::Reflect::get(on, &JsValue::from_str(name))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
}
