fn expansion() -> ::framework_core::Node {
    {
        let __node = (context)
            .child_with_props::<
                Card,
                _,
            >(
                "card",
                {
                    type __Props = <Card as ::framework_core::Component>::Props;
                    #[allow(
                        clippy::needless_update,
                        reason = "every prop may be written"
                    )]
                    let __props = __Props {
                        title: ::core::convert::Into::into("Hi"),
                        ..::core::default::Default::default()
                    };
                    __props
                },
                <Card as ::framework_core::Component>::new,
            );
        let __node = (emphasized)(__node);
        __node
    }
}
