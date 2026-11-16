// dpro: DISM Pro's tests of its menus' check items (PATCHES.md, 5), under the crate's licences:
// the MIT license or the Apache License, Version 2.0, at your option.

//! A menu's check items as a client reaches them, called on the providers in process.
//!
//! The tree is DISM Pro's column menu as dpro-ui builds it: a menu of the hideable columns as
//! check items, then a command.

use accesskit::{Action, Node, NodeId, Role, Toggled, TreeId, TreeInfo, TreeUpdate};
use windows::Win32::UI::Accessibility::*;

use super::providers::{
    Requests, context, control_type, element, is_content, is_control, name, pattern,
};

const WINDOW: NodeId = NodeId(0);
const MENU: NodeId = NodeId(1);
const SHOWN: NodeId = NodeId(2);
const HIDDEN: NodeId = NodeId(3);
const RESET: NodeId = NodeId(4);

fn item(role: Role, label: &str, toggled: Option<Toggled>) -> Node {
    let mut node = Node::new(role);
    node.set_label(label);
    if let Some(toggled) = toggled {
        node.set_toggled(toggled);
    }
    node.add_action(Action::Click);
    node.add_action(Action::Focus);
    node
}

fn tree() -> TreeUpdate {
    let mut window = Node::new(Role::Window);
    window.set_children(vec![MENU]);
    let mut menu = Node::new(Role::Menu);
    menu.set_label("Columns");
    menu.set_children(vec![SHOWN, HIDDEN, RESET]);
    TreeUpdate {
        nodes: vec![
            (WINDOW, window),
            (MENU, menu),
            (
                SHOWN,
                item(Role::MenuItemCheckBox, "Publisher", Some(Toggled::True)),
            ),
            (
                HIDDEN,
                item(Role::MenuItemCheckBox, "Version", Some(Toggled::False)),
            ),
            (RESET, item(Role::MenuItem, "Reset columns", None)),
        ],
        tree: Some(TreeInfo::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus: SHOWN,
    }
}

#[test]
fn a_check_item_is_a_menu_item_that_toggles() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    assert_eq!(
        control_type(&element(&context, MENU)),
        UIA_MenuControlTypeId
    );
    for (id, label, state) in [
        (SHOWN, "Publisher", ToggleState_On),
        (HIDDEN, "Version", ToggleState_Off),
    ] {
        let item = element(&context, id);
        assert_eq!(name(&item), label);
        // As in Windows' own menus: a menu item, not a check box, in both views.
        assert_eq!(control_type(&item), UIA_MenuItemControlTypeId, "{label}");
        assert!(is_control(&item) && is_content(&item), "{label}");
        let toggle: IToggleProvider =
            pattern(&item, UIA_TogglePatternId).expect("a check item toggles");
        assert_eq!(unsafe { toggle.ToggleState() }.unwrap(), state, "{label}");
    }

    // Toggling clicks the item; the app hides or shows the column.
    let toggle: IToggleProvider = pattern(&element(&context, SHOWN), UIA_TogglePatternId).unwrap();
    unsafe { toggle.Toggle() }.unwrap();
    assert_eq!(requests.take(), [(Action::Click, None, SHOWN)]);
}

#[test]
fn a_command_is_a_menu_item_that_invokes() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    let reset = element(&context, RESET);
    assert_eq!(control_type(&reset), UIA_MenuItemControlTypeId);
    assert!(pattern::<IToggleProvider>(&reset, UIA_TogglePatternId).is_none());
    let invoke: IInvokeProvider = pattern(&reset, UIA_InvokePatternId).expect("a command");
    unsafe { invoke.Invoke() }.unwrap();
    assert_eq!(requests.take(), [(Action::Click, None, RESET)]);
}
