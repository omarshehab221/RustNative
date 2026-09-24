//! The reference screen: one ordinary settings screen — a heading, wrapping
//! prose, a tab strip, labelled fields, and actions — that the layout
//! conformance suite checks at several text scales, pseudo-localized, and
//! mirrored (`PLAN.md` Milestone 41), and that `examples/reference-app`
//! runs on a real host for the published comparison.

use framework_core::layout::LayoutDirection;
use framework_core::localization::pseudo_localize;
use framework_core::{
    Alignment, ColumnStyle, Component, EdgeInsets, Event, LayoutStyle, Node, RowStyle, SizeMode,
};

/// How the reference screen is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Variant {
    /// Every string pseudo-localized.
    pub pseudo: bool,
    /// Laid out right to left.
    pub right_to_left: bool,
}

/// The reference screen.
#[derive(Debug)]
pub struct ReferenceScreen {
    variant: Variant,
}

impl ReferenceScreen {
    fn text(&self, text: &str) -> String {
        if self.variant.pseudo { pseudo_localize(text) } else { text.to_owned() }
    }
}

impl Component for ReferenceScreen {
    type Props = Variant;
    type Message = ();

    fn new(variant: Variant) -> Self {
        Self { variant }
    }
    fn props(&self) -> &Variant {
        &self.variant
    }
    fn set_props(&mut self, variant: Variant) {
        self.variant = variant;
    }

    fn view(&self) -> Node {
        let field = |key: &str, label: &str, value: &str| {
            Node::column_with_layout(
                format!("{key}-row"),
                [
                    Node::label(format!("{key}-label"), self.text(label)),
                    Node::text_input(format!("{key}-input"), self.text(value)),
                ],
                LayoutStyle::default(),
                ColumnStyle::new().padding(EdgeInsets::all(0)).gap(4),
            )
        };
        let direction =
            if self.variant.right_to_left { LayoutDirection::Rtl } else { LayoutDirection::Ltr };
        Node::column_with_layout(
            "screen",
            [
                Node::label("heading", self.text("Account settings")),
                Node::label(
                    "intro",
                    self.text(
                        "Changes to your name and address are saved when you choose Save. \
                         Nothing is shared until you confirm.",
                    ),
                ),
                Node::tab_bar(
                    "sections",
                    [self.text("Profile"), self.text("Privacy")],
                    0,
                    LayoutStyle::default(),
                ),
                field("name", "Full name", "Ada Lovelace"),
                field("email", "Email address", "ada@example.com"),
                Node::row_with_layout(
                    "actions",
                    [
                        Node::button("cancel", self.text("Cancel")),
                        Node::button("save", self.text("Save")),
                    ],
                    LayoutStyle::default(),
                    RowStyle::new()
                        .padding(EdgeInsets::all(0))
                        .gap(8)
                        .align_items(Alignment::Center),
                ),
            ],
            LayoutStyle::new().height(SizeMode::Fill).direction(direction),
            ColumnStyle::new().padding(EdgeInsets::all(16)).gap(12),
        )
    }

    fn update(&mut self, _: Event) {}
}
