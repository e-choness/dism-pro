// dpro: DISM Pro's helpers for its in-process provider tests (PATCHES.md, 4 and 5), under the
// crate's licences: the MIT license or the Apache License, Version 2.0, at your option.

//! Providers called the way a client calls them, in process: no window and no UI Automation core,
//! so the tests that use these run under Wine (T2) as well as on Windows (T3).

use accesskit::{ActionData, ActionHandler, ActionRequest, NodeId, TreeId, TreeUpdate};
use accesskit_consumer::Tree;
use std::{
    ffi::c_void,
    sync::{Arc, Mutex},
};
use windows::{
    Win32::{
        Foundation::HWND,
        System::{
            Com::SAFEARRAY,
            Ole::*,
            Variant::{VARIANT, VT_BOOL, VT_BSTR, VT_I4},
        },
        UI::Accessibility::*,
    },
    core::*,
};

use crate::context::{ActionHandlerWrapper, Context};

/// Every request the providers send.
#[derive(Clone, Default)]
pub(super) struct Requests(Arc<Mutex<Vec<ActionRequest>>>);

impl ActionHandler for Requests {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.lock().unwrap().push(request);
    }
}

impl Requests {
    pub(super) fn take(&self) -> Vec<(accesskit::Action, Option<ActionData>, NodeId)> {
        self.0
            .lock()
            .unwrap()
            .drain(..)
            .map(|r| (r.action, r.data, r.target_node))
            .collect()
    }
}

/// `tree`'s context, with no window: these providers never ask for one.
pub(super) fn context(requests: &Requests, tree: TreeUpdate) -> Arc<Context> {
    Context::new(
        HWND::default().into(),
        Tree::new(tree, true),
        Arc::new(ActionHandlerWrapper::new(requests.clone())),
        false,
    )
}

pub(super) fn element(context: &Arc<Context>, id: NodeId) -> IRawElementProviderSimple {
    let full = context
        .read_tree()
        .state()
        .node_by_tree_local_id(id, TreeId::ROOT)
        .unwrap()
        .id();
    context.get_or_create_platform_node(full).into_interface()
}

pub(super) fn pattern<T: Interface>(
    element: &IRawElementProviderSimple,
    id: UIA_PATTERN_ID,
) -> Option<T> {
    // A pattern the node doesn't have is a null provider.
    unsafe { element.GetPatternProvider(id) }
        .ok()
        .map(|p| p.cast().unwrap())
}

pub(super) fn property(element: &IRawElementProviderSimple, id: UIA_PROPERTY_ID) -> VARIANT {
    unsafe { element.GetPropertyValue(id) }.unwrap()
}

/// A string property's value; `None` if it isn't a string.
pub(super) fn text(value: &VARIANT) -> Option<String> {
    // SAFETY: the union holds a BSTR when the type says so.
    unsafe {
        let value = &value.Anonymous.Anonymous;
        (value.vt == VT_BSTR).then(|| value.Anonymous.bstrVal.to_string())
    }
}

/// An int property's value, read as the union holds it: Wine has no `VariantToInt32`.
pub(super) fn int(value: &VARIANT) -> Option<i32> {
    // SAFETY: the union holds an i32 when the type says so.
    unsafe {
        let value = &value.Anonymous.Anonymous;
        (value.vt == VT_I4).then(|| value.Anonymous.lVal)
    }
}

/// A bool property's value.
pub(super) fn flag(value: &VARIANT) -> Option<bool> {
    // SAFETY: the union holds a VARIANT_BOOL when the type says so.
    unsafe {
        let value = &value.Anonymous.Anonymous;
        (value.vt == VT_BOOL).then(|| value.Anonymous.boolVal.as_bool())
    }
}

pub(super) fn name(element: &IRawElementProviderSimple) -> String {
    text(&property(element, UIA_NamePropertyId)).unwrap()
}

pub(super) fn control_type(element: &IRawElementProviderSimple) -> UIA_CONTROLTYPE_ID {
    UIA_CONTROLTYPE_ID(int(&property(element, UIA_ControlTypePropertyId)).unwrap())
}

pub(super) fn is_content(element: &IRawElementProviderSimple) -> bool {
    flag(&property(element, UIA_IsContentElementPropertyId)).unwrap()
}

pub(super) fn is_control(element: &IRawElementProviderSimple) -> bool {
    flag(&property(element, UIA_IsControlElementPropertyId)).unwrap()
}

/// A SAFEARRAY of providers, taken.
pub(super) fn elements(array: *mut SAFEARRAY) -> Vec<IRawElementProviderSimple> {
    let mut out = Vec::new();
    if array.is_null() {
        return out;
    }
    unsafe {
        let (lower, upper) = (
            SafeArrayGetLBound(array, 1).unwrap(),
            SafeArrayGetUBound(array, 1).unwrap(),
        );
        for i in lower..=upper {
            let mut item: Option<IUnknown> = None;
            SafeArrayGetElement(array, &i, (&raw mut item).cast::<c_void>()).unwrap();
            out.push(item.unwrap().cast().unwrap());
        }
        SafeArrayDestroy(array).unwrap();
    }
    out
}

pub(super) fn names(array: *mut SAFEARRAY) -> Vec<String> {
    elements(array).iter().map(name).collect()
}
