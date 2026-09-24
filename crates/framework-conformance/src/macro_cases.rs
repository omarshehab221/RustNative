//! The `rsx!` spelling of every syntax-equivalence case — markup inside a
//! `.rs` file.

use framework_core::{ComponentContext, Cursor, Node, rsx};

use crate::syntax::{self, Card, values};

/// Every case, by name, in the same order as the builder cases.
#[must_use]
pub fn cases() -> Vec<(&'static str, Node)> {
    vec![
        ("label", rsx! { <Label key="greeting" text="Hello" /> }),
        (
            "label_layout",
            rsx! {
                <Label
                    key="sized"
                    text="Sized"
                    width={values::WIDTH}
                    height={values::HEIGHT}
                    margin={values::MARGIN}
                    align_self={values::CENTER}
                    constraints={syntax::constraints()}
                    direction={values::RTL}
                />
            },
        ),
        ("button", rsx! { <Button key="save" text="Save" /> }),
        ("text_input", rsx! { <TextInput key="name" value="Ada" /> }),
        ("canvas", rsx! { <Canvas key="swatch" draw_list={syntax::drawing()} /> }),
        ("surface", rsx! { <Surface key="scene" /> }),
        ("tab_bar", rsx! { <TabBar key="tabs" labels={["One", "Two"]} selected=1 /> }),
        (
            "column",
            rsx! {
                <Column
                    key="stack"
                    padding={values::PADDING}
                    gap=4
                    align_items={values::CENTER}
                    overflow={values::SCROLL}
                >
                    <Label key="a" text="A" />
                    <Label key="b" text="B" />
                </Column>
            },
        ),
        (
            "row",
            rsx! {
                <Row key="line" gap=2>
                    <Button key="ok" text="OK" />
                    <Button key="cancel" text="Cancel" />
                </Row>
            },
        ),
        (
            "virtual_list",
            rsx! {
                <VirtualList key="rows" list={syntax::list()}>
                    for index in 0..3 {
                        <Label key={format!("row-{index}")} text={format!("Row {index}")} item_index={index} />
                    }
                </VirtualList>
            },
        ),
        (
            "accessibility",
            rsx! { <Button key="icon" text="\u{1F4BE}" accessibility={syntax::accessibility()} /> },
        ),
        ("style", rsx! { <Label key="styled" text="Styled" style={syntax::visual()} /> }),
        ("input", rsx! { <Column key="pad" input={syntax::interest()}></Column> }),
        ("opacity", rsx! { <Label key="faint" text="Faint" opacity=0.5 /> }),
        ("transition", rsx! { <Column key="panel" transition={syntax::slide()}></Column> }),
        ("item_index", rsx! { <Label key="item" text="Item" item_index=7 /> }),
        ("command", rsx! { <Button key="save" text="Save" command={syntax::SAVE} /> }),
        ("cursor", rsx! { <Label key="link" text="Link" cursor={Cursor::Pointer} /> }),
        ("disabled", rsx! { <Button key="off" text="Off" disabled /> }),
        ("hidden", rsx! { <Label key="gone" text="Gone" hidden={true} /> }),
        ("spread", rsx! { <Label key="loud" text="Loud" ..{syntax::emphasized} /> }),
        ("control_flow", control_flow(true, 2, &["x", "y"], Some("extra"))),
        ("control_flow_else", control_flow(false, 0, &[], None)),
    ]
}

/// The same children as `builder_cases::control_flow`, as markup writes
/// them: the constructs markup is good at.
#[must_use]
pub fn control_flow(show: bool, count: u8, items: &[&str], extra: Option<&str>) -> Node {
    rsx! {
        <Column key="flow">
            if show {
                <Label key="shown" text="Shown" />
            } else {
                <Label key="hidden-note" text="Nothing to show" />
            }
            match count {
                0 => <Label key="count" text="none" />,
                1 => <Label key="count" text="one" />,
                _ => { <Label key="count" text="many" /> }
            }
            for item in items {
                <Label key={*item} text={*item} />
            }
            <>
                <Label key="first" text="1" />
                <Label key="second" text="2" />
            </>
            {extra.map(|text| rsx! { <Label key="extra" text={text} /> })}
        </Column>
    }
}

/// A component element: `in context,` names the context it composes
/// through.
pub fn components(context: &mut ComponentContext<'_, ()>) -> Node {
    rsx! {
        in context,
        <Column key="cards">
            <Card key="card" title="Welcome" highlighted />
        </Column>
    }
}
