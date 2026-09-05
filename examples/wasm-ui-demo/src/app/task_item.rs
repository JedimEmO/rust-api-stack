use super::*;

pub(super) fn render_task_item(app: Arc<App>, task: Task) -> Dom {
    let task_id = task.id.clone();
    let (_priority_color, _priority_bg, priority_mark) = match task.priority {
        TaskPriority::High => ("text-red-400", "bg-red-900 bg-opacity-20", "H"),
        TaskPriority::Medium => (
            "text-candlelight-400",
            "bg-candlelight-900 bg-opacity-20",
            "M",
        ),
        TaskPriority::Low => ("text-apple-400", "bg-apple-900 bg-opacity-20", "L"),
    };

    html!("div", {
        .class("glass")
        .apply(|b| dwclass!(b, "p-6 rounded-xl hover:shadow-2xl transition-all"))
        .style("cursor", "pointer")
        .style("border", "1px solid rgba(255, 255, 255, 0.1)")
        .event(clone!(app, task => move |_: events::Click| {
            app.selected_task.set(Some(task.clone()));
        }))
        .child(html!("div", {
            .apply(|b| dwclass!(b, "flex gap-4"))
            .children(&mut [
                html!("div", {
                    .apply(|b| dwclass!(b, "flex"))
                    .style("align-items", "center")
                    .child(html!("input" => web_sys::HtmlInputElement, {
                        .apply(|b| dwclass!(b, "w-5 h-5 rounded bg-bunker-800 border-bunker-600 text-picton-blue-500"))
                        .style("cursor", "pointer")
                        .attr("type", "checkbox")
                        .prop("checked", task.completed)
                        .event(clone!(app, task_id => move |e: events::Change| {
                            e.stop_propagation();
                            App::toggle_task_completion(app.clone(), task_id.clone());
                        }))
                    }))
                }),

                html!("div", {
                    .apply(|b| dwclass!(b, "flex-1"))
                    .style("min-width", "0")
                    .children(&mut [
                        html!("div", {
                            .apply(|b| dwclass!(b, "flex justify-between"))
                            .style("align-items", "flex-start")
                            .children(&mut [
                                html!("h4", {
                                    .apply(|b| dwclass!(b, "text-lg font-semibold text-bunker-100"))
                                    .style_signal("text-decoration", Mutable::new(task.completed).signal().map(|completed| {
                                        if completed { "line-through" } else { "none" }
                                    }))
                                    .style_signal("opacity", Mutable::new(task.completed).signal().map(|completed| {
                                        if completed { "0.5" } else { "1" }
                                    }))
                                    .text(&task.title)
                                }),

                                html!("span", {
                                    .class(match task.priority {
                                        TaskPriority::High => "text-red-400",
                                        TaskPriority::Medium => "text-candlelight-400",
                                        TaskPriority::Low => "text-apple-400",
                                    })
                                    .style("background-color", match task.priority {
                                        TaskPriority::High => "rgba(127, 29, 29, 0.2)",
                                        TaskPriority::Medium => "rgba(180, 83, 9, 0.2)",
                                        TaskPriority::Low => "rgba(21, 128, 61, 0.2)",
                                    })
                                    .apply(|b| dwclass!(b, "rounded-full text-xs font-medium flex gap-1"))
                                    .style("padding", "0.25rem 0.75rem")
                                    .style("align-items", "center")
                                    .children(&mut [
                                        html!("span", {
                                            .text(priority_mark)
                                        }),
                                        html!("span", {
                                            .class(match task.priority {
                                                TaskPriority::High => "text-red-400",
                                                TaskPriority::Medium => "text-candlelight-400",
                                                TaskPriority::Low => "text-apple-400",
                                            })
                                            .text(&format!("{:?}", task.priority))
                                        }),
                                    ])
                                }),
                            ])
                        }),

                        html!("p", {
                            .apply(|b| dwclass!(b, "text-sm text-bunker-400"))
                            .style("margin-top", "0.5rem")
                            .style_signal("opacity", Mutable::new(task.completed).signal().map(|completed| {
                                if completed { "0.5" } else { "1" }
                            }))
                            .text(&task.description)
                        }),

                        html!("div", {
                            .apply(|b| dwclass!(b, "flex gap-4 text-xs text-bunker-500"))
                            .style("margin-top", "0.75rem")
                            .children(&mut [
                                html!("span", {
                                    .apply(|b| dwclass!(b, "flex gap-1"))
                                    .style("align-items", "center")
                                    .children(&mut [
                                        html!("span", {
                                            .text("Created")
                                        }),
                                        html!("span", {
                                            .text(timestamp_date(&task.created_at))
                                        }),
                                    ])
                                }),
                            ])
                        }),
                    ])
                }),

                html!("button", {
                    .apply(|b| dwclass!(b, "text-red-400 hover:text-red-300 text-sm font-medium rounded-lg transition-all"))
                    .style("padding", "0.25rem 0.75rem")
                    .text("Delete")
                    .event(clone!(app, task_id => move |e: events::Click| {
                        e.stop_propagation();
                        App::delete_task(app.clone(), task_id.clone());
                    }))
                }),
            ])
        }))
    })
}
