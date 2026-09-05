use super::*;

pub(super) fn render_stats_card(stats: &DashboardStats) -> Dom {
    html!("div", {
        .class("glass")
        .apply(|b| dwclass!(b, "rounded-2xl p-8"))
        .children(&mut [
            html!("h3", {
                .apply(|b| dwclass!(b, "text-2xl font-bold text-bunker-100"))
                .style("margin-bottom", "2rem")
                .text("Dashboard Overview")
            }),

            html!("div", {
                .apply(|b| dwclass!(b, "grid grid-cols-2 gap-6"))
                .children(&mut [
                    // Total Tasks
                    html!("div", {
                        .apply(|b| dwclass!(b, "p-6 rounded-xl"))
                        .style("background", "linear-gradient(to bottom right, #2563eb, #1e40af)")
                        .style("box-shadow", "0 8px 32px rgba(59, 130, 246, 0.2)")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex justify-between"))
                                .style("align-items", "flex-start")
                                .children(&mut [
                                    html!("div", {
                                        .children(&mut [
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-3xl font-bold"))
                                                .style("color", "white")
                                                .text(&stats.total_tasks.to_string())
                                            }),
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-sm text-picton-blue-200"))
                                                .style("margin-top", "0.25rem")
                                                .text("Total Tasks")
                                            }),
                                        ])
                                    }),
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "text-picton-blue-300"))
                                        .text("All")
                                        .style("font-size", "1.5rem")
                                    }),
                                ])
                            }),
                        ])
                    }),

                    // Completed Tasks
                    html!("div", {
                        .apply(|b| dwclass!(b, "p-6 rounded-xl"))
                        .style("background", "linear-gradient(to bottom right, #16a34a, #15803d)")
                        .style("box-shadow", "0 8px 32px rgba(34, 197, 94, 0.2)")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex justify-between"))
                                .style("align-items", "flex-start")
                                .children(&mut [
                                    html!("div", {
                                        .children(&mut [
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-3xl font-bold"))
                                                .style("color", "white")
                                                .text(&stats.completed_tasks.to_string())
                                            }),
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-sm text-apple-200"))
                                                .style("margin-top", "0.25rem")
                                                .text("Completed")
                                            }),
                                        ])
                                    }),
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "text-apple-300"))
                                        .text("Done")
                                        .style("font-size", "1.5rem")
                                    }),
                                ])
                            }),
                        ])
                    }),

                    // Pending Tasks
                    html!("div", {
                        .apply(|b| dwclass!(b, "p-6 rounded-xl"))
                        .style("background", "linear-gradient(to bottom right, #d97706, #b45309)")
                        .style("box-shadow", "0 8px 32px rgba(251, 191, 36, 0.2)")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex justify-between"))
                                .style("align-items", "flex-start")
                                .children(&mut [
                                    html!("div", {
                                        .children(&mut [
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-3xl font-bold"))
                                                .style("color", "white")
                                                .text(&stats.pending_tasks.to_string())
                                            }),
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-sm text-candlelight-200"))
                                                .style("margin-top", "0.25rem")
                                                .text("Pending")
                                            }),
                                        ])
                                    }),
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "text-candlelight-300"))
                                        .text("⏳")
                                        .style("font-size", "1.5rem")
                                    }),
                                ])
                            }),
                        ])
                    }),

                    // High Priority Tasks
                    html!("div", {
                        .apply(|b| dwclass!(b, "p-6 rounded-xl"))
                        .style("background", "linear-gradient(to bottom right, #dc2626, #991b1b)")
                        .style("box-shadow", "0 8px 32px rgba(239, 68, 68, 0.2)")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex justify-between"))
                                .style("align-items", "flex-start")
                                .children(&mut [
                                    html!("div", {
                                        .children(&mut [
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-3xl font-bold"))
                                                .style("color", "white")
                                                .text(&stats.high_priority_tasks.to_string())
                                            }),
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-sm text-red-200"))
                                                .style("margin-top", "0.25rem")
                                                .text("High Priority")
                                            }),
                                        ])
                                    }),
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "text-red-300"))
                                        .text("High")
                                        .style("font-size", "1.5rem")
                                    }),
                                ])
                            }),
                        ])
                    }),
                ])
            }),
        ])
    })
}
