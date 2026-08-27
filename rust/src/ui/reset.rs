//! Putting the browser back to how it was before the tool was ever opened.
//!
//! The project itself lives in local storage, but a page can accumulate more
//! than that: a session store, whatever the Cache API is holding, and any
//! indexed databases. All of it goes, and only then does the page reload, so
//! what comes back is a new project rather than a half cleared one.
//!
//! None of the three asynchronous stores is guaranteed to exist, and asking for
//! one the browser does not have must not leave the reload waiting forever, so
//! every step is counted in and out and the reload happens when the count
//! reaches nothing.

use std::cell::Cell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::ui::window;

/// Counts the parts of a reset still running. It starts at one for the walk
/// that sets the rest going, so a browser with nothing to clear still reloads
/// exactly once.
#[derive(Clone)]
struct Waiter(Rc<Cell<i32>>);

impl Waiter {
    fn new() -> Waiter {
        Waiter(Rc::new(Cell::new(1)))
    }

    fn hold(&self) {
        self.0.set(self.0.get() + 1);
    }

    fn release(&self) {
        self.0.set(self.0.get() - 1);
        if self.0.get() == 0 {
            reload();
        }
    }

    /// A callback that releases the first time it is called and does nothing
    /// afterwards, because a request can report both blocked and then done.
    fn once(&self) -> Closure<dyn FnMut(JsValue)> {
        self.hold();
        let me = self.clone();
        let fired = Rc::new(Cell::new(false));
        Closure::wrap(Box::new(move |_: JsValue| {
            if fired.replace(true) {
                return;
            }
            me.release();
        }) as Box<dyn FnMut(JsValue)>)
    }

    fn watch(&self, promise: &js_sys::Promise) {
        let cb = self.once();
        let _ = promise.then2(&cb, &cb);
        cb.forget();
    }
}

fn reload() {
    let _ = window().location().reload();
}

/// Reads a property off an object, or nothing if it is missing or not there to
/// be read. Both of the stores below are optional, and asking for one on a
/// browser without it must not be an error.
fn get(target: &JsValue, key: &str) -> Option<JsValue> {
    let value = js_sys::Reflect::get(target, &JsValue::from_str(key)).ok()?;
    if value.is_undefined() || value.is_null() {
        None
    } else {
        Some(value)
    }
}

/// Calls a method by name with however many arguments were given.
fn call(target: &JsValue, method: &str, args: &[JsValue]) -> Option<JsValue> {
    let f = get(target, method)?.dyn_into::<js_sys::Function>().ok()?;
    let list = js_sys::Array::new();
    for arg in args {
        list.push(arg);
    }
    js_sys::Reflect::apply(&f, target, &list).ok()
}

/// Everything, then a reload. The two synchronous stores go first so that a
/// browser that refuses the rest still comes back with the project gone.
pub fn everything() {
    let win = window();
    if let Ok(Some(store)) = win.local_storage() {
        let _ = store.clear();
    }
    if let Ok(Some(store)) = win.session_storage() {
        let _ = store.clear();
    }
    let waiter = Waiter::new();
    clear_caches(&waiter);
    clear_databases(&waiter);
    waiter.release();
    // A store that never answers must not leave the page sitting on a reset
    // that looks like it did nothing, so the reload happens regardless once
    // everything has had its moment.
    let cb = Closure::wrap(Box::new(reload) as Box<dyn FnMut()>);
    let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
        cb.as_ref().unchecked_ref(),
        2500,
    );
    cb.forget();
}

fn clear_caches(waiter: &Waiter) {
    let caches = match get(window().as_ref(), "caches") {
        Some(c) => c,
        None => return,
    };
    let keys = match call(&caches, "keys", &[]).and_then(|v| v.dyn_into::<js_sys::Promise>().ok()) {
        Some(p) => p,
        None => return,
    };
    waiter.hold();
    let outer = waiter.clone();
    let cb = Closure::wrap(Box::new(move |names: JsValue| {
        if let Ok(names) = names.dyn_into::<js_sys::Array>() {
            for name in names.iter() {
                if let Some(p) = call(&caches, "delete", &[name])
                    .and_then(|v| v.dyn_into::<js_sys::Promise>().ok())
                {
                    outer.watch(&p);
                }
            }
        }
        outer.release();
    }) as Box<dyn FnMut(JsValue)>);
    let _ = keys.then2(&cb, &cb);
    cb.forget();
}

/// Indexed databases, on the browsers that will say which ones exist. Deleting
/// one answers on the request rather than on a promise, so each is watched by
/// its own handlers instead.
fn clear_databases(waiter: &Waiter) {
    let factory = match get(window().as_ref(), "indexedDB") {
        Some(f) => f,
        None => return,
    };
    let listed = match call(&factory, "databases", &[])
        .and_then(|v| v.dyn_into::<js_sys::Promise>().ok())
    {
        Some(p) => p,
        None => return,
    };
    waiter.hold();
    let outer = waiter.clone();
    let cb = Closure::wrap(Box::new(move |list: JsValue| {
        if let Ok(list) = list.dyn_into::<js_sys::Array>() {
            for entry in list.iter() {
                let name = match get(&entry, "name") {
                    Some(n) => n,
                    None => continue,
                };
                if let Some(request) = call(&factory, "deleteDatabase", &[name]) {
                    let done = outer.once();
                    for event in ["onsuccess", "onerror", "onblocked"] {
                        let _ = js_sys::Reflect::set(
                            &request,
                            &JsValue::from_str(event),
                            done.as_ref(),
                        );
                    }
                    done.forget();
                }
            }
        }
        outer.release();
    }) as Box<dyn FnMut(JsValue)>);
    let _ = listed.then2(&cb, &cb);
    cb.forget();
}
