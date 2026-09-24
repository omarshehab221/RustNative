fn expansion() -> ::framework_core::Node {
    {
        let __layout = <::framework_core::LayoutStyle as ::core::default::Default>::default()
            .width(SizeMode::Fixed(80));
        let __node = ::framework_core::Node::label_with_layout("title", title, __layout);
        let __node = __node.with_opacity(0.5);
        let __node = __node.disabled(true);
        __node
    }
}
