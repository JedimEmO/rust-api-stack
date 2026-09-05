use super::*;

pub(super) fn render_task_list(app: Arc<App>) -> Dom {
    html!("div", {
        .class("glass")
        .apply(|b| dwclass!(b, "rounded-2xl p-8"))
        .children(&mut [
            html!("div", {
                .apply(|b| dwclass!(b, "flex justify-between"))
                .style("align-items", "center")
                .style("margin-bottom", "2rem")
                .children(&mut [
                    html!("h3", {
                        .apply(|b| dwclass!(b, "text-2xl font-bold text-bunker-100"))
                        .text("Your Tasks")
                    }),
                    html!("div", {
                        .apply(|b| dwclass!(b, "text-sm text-bunker-400"))
                        .text_signal(app.tasks.signal_vec_cloned().len().map(|len| {
                            format!("{} task{}", len, if len == 1 { "" } else { "s" })
                        }))
                    }),
                ])
            }),

            html!("div", {
                .style("display", "flex")
                .style("flex-direction", "column")
                .style("gap", "1rem")
                .children_signal_vec(app.tasks.signal_vec_cloned()
                    .map(clone!(app => move |task| {
                        render_task_item(app.clone(), task)
                    })))
            }),

            // Empty state
            html!("div", {
                .apply(|b| dwclass!(b, "text-center"))
                .style("padding", "3rem 0")
                .visible_signal(app.tasks.signal_vec_cloned().len().map(|len| len == 0))
                .children(&mut [
                    html!("div", {
                        .apply(|b| dwclass!(b, "text-2xl font-semibold text-bunker-300"))
                        .style("margin-bottom", "1rem")
                        .text("No Tasks")
                    }),
                    html!("p", {
                        .apply(|b| dwclass!(b, "text-bunker-400 text-lg"))
                        .text("No tasks yet. Create your first task!")
                    }),
                ])
            }),
        ])
    })
}
