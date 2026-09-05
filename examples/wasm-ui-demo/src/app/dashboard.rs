use super::*;

pub(super) fn render_dashboard(app: Arc<App>) -> Dom {
    html!("div", {
        .class(&*STYLES)
        .style("min-height", "100vh")
        .style("background", "linear-gradient(to bottom, #0a0a0a, #000000)")
        .children(&mut [
            // Header
            html!("nav", {
                .class("glass")
                .apply(|b| dwclass!(b, "sticky top-0"))
                .style("z-index", "50")
                .child(html!("div", {
                    .apply(|b| dwclass!(b, "max-w-7xl p-4"))
                    .style("margin", "0 auto")
                    .child(html!("div", {
                        .apply(|b| dwclass!(b, "flex justify-between"))
                        .style("align-items", "center")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex gap-3"))
                                .style("align-items", "center")
                                .children(&mut [
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "w-10 h-10 rounded-lg flex justify-center"))
                                        .style("background", "linear-gradient(to bottom right, #3b82f6, #8b5cf6)")
                                        .style("align-items", "center")
                                        .child(html!("span", {
                                            .apply(|b| dwclass!(b, "font-bold text-lg"))
                                            .style("color", "white")
                                            .text("T")
                                        }))
                                    }),
                                    html!("h1", {
                                        .apply(|b| dwclass!(b, "text-2xl font-bold"))
                                        .style("background", "linear-gradient(to right, #60a5fa, #a78bfa)")
                                        .style("background-clip", "text")
                                        .style("-webkit-background-clip", "text")
                                        .style("color", "transparent")
                                        .text("Task Manager")
                                    }),
                                ])
                            }),

                            html!("button", {
                                .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300 rounded-lg transition-all border border-bunker-700"))
                                .style("padding", "0.5rem 1rem")
                                .style("background-color", "rgba(31, 41, 55, 0.5)")
                                .text("Sign Out")
                                .event(clone!(app => move |_: events::Click| {
                                    App::logout(app.clone());
                                }))
                            }),
                        ])
                    }))
                }))
            }),

            // Main content
            html!("main", {
                .apply(|b| dwclass!(b, "max-w-7xl p-6"))
                .style("margin", "0 auto")
                .style("padding-top", "2rem")
                .style("padding-bottom", "2rem")
                .child(html!("div", {
                    .apply(|b| dwclass!(b, "grid gap-8"))
                    .style("grid-template-columns", "1fr")
                    .children(&mut [
                        // Left column - Stats and Tasks
                        html!("div", {
                            .style("display", "flex")
                            .style("flex-direction", "column")
                            .style("gap", "1.5rem")
                            .children(&mut [
                                // Stats
                                html!("div", {
                                    .child_signal(app.stats.signal_cloned().map(|stats| {
                                        stats.map(|s| render_stats_card(&s))
                                    }))
                                }),

                                // Task list
                                render_task_list(app.clone()),
                            ])
                        }),

                        // Right column - Create form and selected task
                        html!("div", {
                            .style("display", "flex")
                            .style("flex-direction", "column")
                            .style("gap", "1.5rem")
                            .children(&mut [
                                // Create task form
                                render_task_form(app.clone()),

                                // Selected task details
                                html!("div", {
                                    .child_signal(app.selected_task.signal_cloned().map(clone!(app => move |task| {
                                        task.map(|t| {
                                            html!("div", {
                                                .class("glass")
                                                .class("animate-fade-in")
                                                .apply(|b| dwclass!(b, "rounded-2xl p-8"))
                                                .children(&mut [
                                                    html!("div", {
                                                        .apply(|b| dwclass!(b, "flex justify-between"))
                                                        .style("align-items", "center")
                                                        .style("margin-bottom", "2rem")
                                                        .children(&mut [
                                                            html!("h3", {
                                                                .apply(|b| dwclass!(b, "text-2xl font-bold text-bunker-100"))
                                                                .text("Task Details")
                                                            }),
                                                            html!("button", {
                                                                .apply(|b| dwclass!(b, "text-bunker-400 hover:text-bunker-200 text-2xl"))
                                                                .text("×")
                                                                .event(clone!(app => move |_: events::Click| {
                                                                    app.selected_task.set(None);
                                                                }))
                                                            }),
                                                        ])
                                                    }),

                                                    html!("div", {
                                                        .style("display", "flex")
                                                        .style("flex-direction", "column")
                                                        .style("gap", "1.5rem")
                                                        .children(&mut [
                                                            // Title and status
                                                            html!("div", {
                                                                .children(&mut [
                                                                    html!("h4", {
                                                                        .apply(|b| dwclass!(b, "text-xl font-semibold text-bunker-100"))
                                                                        .style("margin-bottom", "0.5rem")
                                                                        .text(&t.title)
                                                                    }),
                                                                    html!("p", {
                                                                        .apply(|b| dwclass!(b, "text-bunker-400"))
                                                                        .text(&t.description)
                                                                    }),
                                                                ])
                                                            }),

                                                            // Meta info
                                                            html!("div", {
                                                                .apply(|b| dwclass!(b, "grid grid-cols-2 gap-4"))
                                                                .children(&mut [
                                                                    html!("div", {
                                                                        .apply(|b| dwclass!(b, "rounded-lg p-4"))
                                                                        .style("background-color", "rgba(31, 41, 55, 0.5)")
                                                                        .children(&mut [
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-xs text-bunker-500"))
                                                                                .style("text-transform", "uppercase")
                                                                                .style("letter-spacing", "0.05em")
                                                                                .text("Task ID")
                                                                            }),
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-sm text-bunker-300 font-mono"))
                                                                                .style("margin-top", "0.25rem")
                                                                                .text(task_id_preview(&t.id))
                                                                                .attr("title", &t.id)
                                                                            }),
                                                                        ])
                                                                    }),

                                                                    html!("div", {
                                                                        .apply(|b| dwclass!(b, "rounded-lg p-4"))
                                                                        .style("background-color", "rgba(31, 41, 55, 0.5)")
                                                                        .children(&mut [
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-xs text-bunker-500"))
                                                                                .style("text-transform", "uppercase")
                                                                                .style("letter-spacing", "0.05em")
                                                                                .text("Status")
                                                                            }),
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-sm font-medium"))
                                                                                .style("margin-top", "0.25rem")
                                                                                .apply(|b| if t.completed {
                                                                                    dwclass!(b, "text-apple-400")
                                                                                } else {
                                                                                    dwclass!(b, "text-candlelight-400")
                                                                                })
                                                                                .text(if t.completed { "Completed" } else { "In Progress" })
                                                                            }),
                                                                        ])
                                                                    }),

                                                                    html!("div", {
                                                                        .apply(|b| dwclass!(b, "rounded-lg p-4"))
                                                                        .style("background-color", "rgba(31, 41, 55, 0.5)")
                                                                        .children(&mut [
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-xs text-bunker-500"))
                                                                                .style("text-transform", "uppercase")
                                                                                .style("letter-spacing", "0.05em")
                                                                                .text("Created")
                                                                            }),
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-sm text-bunker-300"))
                                                                                .style("margin-top", "0.25rem")
                                                                                .text(timestamp_date(&t.created_at))
                                                                            }),
                                                                        ])
                                                                    }),

                                                                    html!("div", {
                                                                        .apply(|b| dwclass!(b, "rounded-lg p-4"))
                                                                        .style("background-color", "rgba(31, 41, 55, 0.5)")
                                                                        .children(&mut [
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-xs text-bunker-500"))
                                                                                .style("text-transform", "uppercase")
                                                                                .style("letter-spacing", "0.05em")
                                                                                .text("Updated")
                                                                            }),
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-sm text-bunker-300"))
                                                                                .style("margin-top", "0.25rem")
                                                                                .text(timestamp_date(&t.updated_at))
                                                                            }),
                                                                        ])
                                                                    }),
                                                                ])
                                                            }),
                                                        ])
                                                    }),
                                                ])
                                            })
                                        })
                                    })))
                                }),
                            ])
                        }),
                    ])
                }))
            }),
        ])
    })
}
