use super::*;

pub(super) fn render_login_form(app: Arc<App>) -> Dom {
    html!("div", {
        .class(&*STYLES)
        .apply(|b| dwclass!(b, "flex justify-center"))
        .style("background", "linear-gradient(to bottom right, #1a1a1a, #0f0f0f, #000000)")
        .style("min-height", "100vh")
        .style("position", "relative")
        .style("overflow", "hidden")
        .children(&mut [
            // Background decoration
            html!("div", {
                .style("position", "absolute")
                .style("top", "-50%")
                .style("right", "-50%")
                .style("width", "200%")
                .style("height", "200%")
                .style("background", "radial-gradient(circle at center, rgba(59, 130, 246, 0.1) 0%, transparent 70%)")
                .style("animation", "rotate 30s linear infinite")
            }),

            html!("div", {
                .apply(|b| dwclass!(b, "flex flex-col justify-center w-full max-w-md p-8"))
                .style("position", "relative")
                .style("z-index", "10")
                .child(html!("div", {
                    .class("glass")
                    .apply(|b| dwclass!(b, "rounded-2xl shadow-2xl p-10"))
                    .children(&mut [
                        html!("h2", {
                            .apply(|b| dwclass!(b, "text-3xl font-bold text-center"))
                            .style("background", "linear-gradient(to right, #60a5fa, #a78bfa)")
                            .style("background-clip", "text")
                            .style("-webkit-background-clip", "text")
                            .style("color", "transparent")
                            .style("margin-bottom", "0.5rem")
                            .text("Welcome Back")
                        }),

                        html!("p", {
                            .apply(|b| dwclass!(b, "text-bunker-400 text-center"))
                            .style("margin-bottom", "2rem")
                            .text("Sign in to manage your tasks")
                        }),

                        html!("div", {
                            .children(&mut [
                                html!("div", {
                                    .style("margin-bottom", "1.5rem")
                                    .children(&mut [
                                        html!("label", {
                                            .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                            .style("display", "block")
                                            .style("margin-bottom", "0.5rem")
                                            .text("Username")
                                        }),
                                        html!("input", {
                                            .apply(|b| dwclass!(b, "w-full p-4 border border-bunker-700 rounded-lg text-bunker-100 focus:border-picton-blue-500"))
                                            .style("background-color", "rgba(24, 24, 27, 0.5)")
                                            .style("outline", "none")
                                            .attr("type", "text")
                                            .attr("placeholder", "Enter your username")
                                            .prop_signal("value", app.username.signal_cloned())
                                            .event(clone!(app => move |_: events::Input| {
                                                let elem = web_sys::window()
                                                    .unwrap()
                                                    .document()
                                                    .unwrap()
                                                    .active_element()
                                                    .unwrap()
                                                    .dyn_into::<web_sys::HtmlInputElement>()
                                                    .unwrap();
                                                app.username.set(elem.value());
                                            }))
                                        }),
                                    ])
                                }),

                                html!("div", {
                                    .style("margin-bottom", "2rem")
                                    .children(&mut [
                                        html!("label", {
                                            .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                            .style("display", "block")
                                            .style("margin-bottom", "0.5rem")
                                            .text("Password")
                                        }),
                                        html!("input", {
                                            .apply(|b| dwclass!(b, "w-full p-4 border border-bunker-700 rounded-lg text-bunker-100 focus:border-picton-blue-500"))
                                            .style("background-color", "rgba(24, 24, 27, 0.5)")
                                            .style("outline", "none")
                                            .attr("type", "password")
                                            .attr("placeholder", "Enter your password")
                                            .prop_signal("value", app.password.signal_cloned())
                                            .event(clone!(app => move |_: events::Input| {
                                                let elem = web_sys::window()
                                                    .unwrap()
                                                    .document()
                                                    .unwrap()
                                                    .active_element()
                                                    .unwrap()
                                                    .dyn_into::<web_sys::HtmlInputElement>()
                                                    .unwrap();
                                                app.password.set(elem.value());
                                            }))
                                        }),
                                    ])
                                }),

                                html!("div", {
                                    .child_signal(app.login_error.signal_cloned().map(|error| {
                                        error.map(|msg| {
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-red-400 text-sm text-center border border-red-800 rounded-lg p-3"))
                                                .style("background-color", "rgba(127, 29, 29, 0.2)")
                                                .style("margin-bottom", "1.5rem")
                                                .text(&msg)
                                            })
                                        })
                                    }))
                                }),

                                html!("button", {
                                    .apply(|b| dwclass!(b, "w-full p-4 font-semibold rounded-lg transition-all"))
                                    .style("color", "white")
                                    .style_signal("background", app.is_loading.signal().map(|loading| {
                                        if !loading { "linear-gradient(135deg, #3b82f6 0%, #8b5cf6 100%)" } else { "#4b5563" }
                                    }))
                                    .style_signal("cursor", app.is_loading.signal().map(|loading| {
                                        if !loading { "pointer" } else { "not-allowed" }
                                    }))
                                    .style("box-shadow", "0 4px 15px rgba(59, 130, 246, 0.3)")
                                    .attr("type", "button")
                                    .prop_signal("disabled", app.is_loading.signal())
                                    .text_signal(app.is_loading.signal().map(|loading| {
                                        if loading { "Signing In..." } else { "Sign In" }
                                    }))
                                    .event(clone!(app => move |_: events::Click| {
                                        App::login(app.clone());
                                    }))
                                }),

                                html!("div", {
                                    .style("margin-top", "2rem")
                                    .apply(|b| dwclass!(b, "text-sm text-bunker-500 text-center"))
                                    .children(&mut [
                                        html!("p", {
                                            .text("Demo credentials:")
                                        }),
                                        html!("p", {
                                            .apply(|b| dwclass!(b, "text-bunker-400"))
                                            .style("margin-top", "0.25rem")
                                            .text("user/password • admin/secret")
                                        }),
                                    ])
                                }),
                            ])
                        }),
                    ])
                }))
            }),
        ])
    })
}
