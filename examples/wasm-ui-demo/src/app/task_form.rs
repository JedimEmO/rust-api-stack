use super::*;

pub(super) fn render_task_form(app: Arc<App>) -> Dom {
    html!("div", {
        .class("glass")
        .apply(|b| dwclass!(b, "rounded-2xl p-8"))
        .children(&mut [
            html!("h3", {
                .apply(|b| dwclass!(b, "text-2xl font-bold text-bunker-100"))
                .style("margin-bottom", "2rem")
                .text("Create New Task")
            }),

            html!("div", {
                .children(&mut [
                    // Title field
                    html!("div", {
                        .style("margin-bottom", "1.5rem")
                        .children(&mut [
                            html!("label", {
                                .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                .style("display", "block")
                                .style("margin-bottom", "0.5rem")
                                .text("Title")
                            }),
                            html!("input", {
                                .apply(|b| dwclass!(b, "w-full p-4 border border-bunker-700 rounded-lg text-bunker-100 focus:border-picton-blue-500 transition-all"))
                                .style("background-color", "rgba(24, 24, 27, 0.5)")
                                .style("outline", "none")
                                .attr("type", "text")
                                .attr("placeholder", "What needs to be done?")
                                .prop_signal("value", app.new_task_title.signal_cloned())
                                .event(clone!(app => move |_: events::Input| {
                                    let elem = web_sys::window()
                                        .unwrap()
                                        .document()
                                        .unwrap()
                                        .active_element()
                                        .unwrap()
                                        .dyn_into::<web_sys::HtmlInputElement>()
                                        .unwrap();
                                    app.new_task_title.set(elem.value());
                                }))
                            }),
                        ])
                    }),

                    // Description field
                    html!("div", {
                        .style("margin-bottom", "1.5rem")
                        .children(&mut [
                            html!("label", {
                                .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                .style("display", "block")
                                .style("margin-bottom", "0.5rem")
                                .text("Description")
                            }),
                            html!("textarea", {
                                .apply(|b| dwclass!(b, "w-full p-4 border border-bunker-700 rounded-lg text-bunker-100 focus:border-picton-blue-500 transition-all"))
                                .style("background-color", "rgba(24, 24, 27, 0.5)")
                                .style("outline", "none")
                                .style("resize", "vertical")
                                .style("min-height", "80px")
                                .attr("placeholder", "Add more details...")
                                .prop_signal("value", app.new_task_description.signal_cloned())
                                .event(clone!(app => move |_: events::Input| {
                                    let elem = web_sys::window()
                                        .unwrap()
                                        .document()
                                        .unwrap()
                                        .active_element()
                                        .unwrap()
                                        .dyn_into::<web_sys::HtmlTextAreaElement>()
                                        .unwrap();
                                    app.new_task_description.set(elem.value());
                                }))
                            }),
                        ])
                    }),

                    // Priority field
                    html!("div", {
                        .style("margin-bottom", "2rem")
                        .children(&mut [
                            html!("label", {
                                .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                .style("display", "block")
                                .style("margin-bottom", "0.5rem")
                                .text("Priority")
                            }),
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex gap-3"))
                                .children(&mut [
                                    html!("button", {
                                        .apply(|b| dwclass!(b, "flex-1 p-3 text-sm font-medium rounded-lg border transition-all"))
                                        .style_signal("background-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Low) { "#16a34a" } else { "#1f2937" }
                                        }))
                                        .style_signal("border-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Low) { "#16a34a" } else { "#374151" }
                                        }))
                                        .style_signal("color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Low) { "white" } else { "#9ca3af" }
                                        }))
                                        .attr("type", "button")
                                        .text("Low")
                                        .event(clone!(app => move |_: events::Click| {
                                            app.new_task_priority.set(TaskPriority::Low);
                                        }))
                                    }),

                                    html!("button", {
                                        .apply(|b| dwclass!(b, "flex-1 p-3 text-sm font-medium rounded-lg border transition-all"))
                                        .style_signal("background-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Medium) { "#d97706" } else { "#1f2937" }
                                        }))
                                        .style_signal("border-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Medium) { "#d97706" } else { "#374151" }
                                        }))
                                        .style_signal("color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Medium) { "white" } else { "#9ca3af" }
                                        }))
                                        .attr("type", "button")
                                        .text("Medium")
                                        .event(clone!(app => move |_: events::Click| {
                                            app.new_task_priority.set(TaskPriority::Medium);
                                        }))
                                    }),

                                    html!("button", {
                                        .apply(|b| dwclass!(b, "flex-1 p-3 text-sm font-medium rounded-lg border transition-all"))
                                        .style_signal("background-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::High) { "#dc2626" } else { "#1f2937" }
                                        }))
                                        .style_signal("border-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::High) { "#dc2626" } else { "#374151" }
                                        }))
                                        .style_signal("color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::High) { "white" } else { "#9ca3af" }
                                        }))
                                        .attr("type", "button")
                                        .text("High")
                                        .event(clone!(app => move |_: events::Click| {
                                            app.new_task_priority.set(TaskPriority::High);
                                        }))
                                    }),
                                ])
                            }),
                        ])
                    }),

                    html!("button", {
                        .apply(|b| dwclass!(b, "w-full p-4 font-semibold rounded-lg transition-all"))
                        .style("color", "white")
                        .style_signal("background", app.new_task_title.signal_ref(|t| {
                            if !t.is_empty() { "linear-gradient(135deg, #3b82f6 0%, #8b5cf6 100%)" } else { "#374151" }
                        }))
                        .style_signal("cursor", app.new_task_title.signal_ref(|t| {
                            if !t.is_empty() { "pointer" } else { "not-allowed" }
                        }))
                        .style_signal("box-shadow", app.new_task_title.signal_ref(|t| {
                            if !t.is_empty() { "0 4px 15px rgba(59, 130, 246, 0.3)" } else { "none" }
                        }))
                        .attr("type", "button")
                        .prop_signal("disabled", app.new_task_title.signal_ref(|t| t.is_empty()))
                        .text("Create Task")
                        .event(clone!(app => move |_: events::Click| {
                            App::create_task(app.clone());
                        }))
                    }),
                ])
            }),
        ])
    })
}
