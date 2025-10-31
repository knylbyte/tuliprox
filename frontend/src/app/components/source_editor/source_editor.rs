use crate::app::components::source_editor::editor_model::{Block, BlockType, Connection};
use crate::app::components::source_editor::rules::can_connect;
use crate::app::components::source_editor::sidebar::SourceEditorSidebar;
use crate::html_if;
use web_sys::{HtmlElement, MouseEvent};
use yew::prelude::*;
use yew_i18n::use_translation;

const PORT_INACTIVE: u32 = 0;
const PORT_VALID: u32 = 1;
const PORT_INVALID: u32 = 2;

// ----------------- Component -----------------
#[function_component]
pub fn SourceEditor() -> Html {
    let translate = use_translation();
    let canvas_ref = use_node_ref();
    let blocks = use_state(|| Vec::<Block>::new());
    let connections = use_state(|| Vec::<Connection>::new());
    let next_id = use_state(|| 1usize);

    // Dragging state
    let dragging_block = use_state(|| None as Option<usize>);
    let drag_offset = use_state(|| (0.0f32, 0.0f32));

    // Pending line for live connection
    let pending_line = use_state(|| None as Option<((f32, f32), (f32, f32))>);
    let pending_connection = use_state(|| None as Option<usize>);

    // Delete mode toggle
    let delete_mode = use_state(|| false);

    // ----------------- Drag Start from Sidebar -----------------
    let on_drag_start = Callback::from(|e: DragEvent| {
        if let Some(target) = e.target_dyn_into::<HtmlElement>() {
            let block_type = target.get_attribute("data-block-type").unwrap_or_default();
            e.data_transfer().unwrap().set_data("text/plain", &block_type).unwrap();
        }
    });

    // ----------------- Drop on Canvas -----------------
    let on_drop = {
        let blocks = blocks.clone();
        let next_id = next_id.clone();
        let canvas_ref = canvas_ref.clone();
        Callback::from(move |e: DragEvent| {
            e.prevent_default();
            if let Some(canvas) = canvas_ref.cast::<HtmlElement>() {
                if let Some(data) = e.data_transfer().unwrap().get_data("text/plain").ok() {
                    let rect = canvas.get_bounding_client_rect();
                    let x = e.client_x() as f32 - rect.left() as f32;
                    let y = e.client_y() as f32 - rect.top() as f32;

                    let block_type = BlockType::from(data.as_str());

                    let mut current_blocks = (*blocks).clone();
                    current_blocks.push(Block {
                        id: *next_id,
                        block_type,
                        position: (x, y),
                    });
                    blocks.set(current_blocks);
                    next_id.set(*next_id + 1);
                }
            }
        })
    };
    let on_drag_over = Callback::from(|e: DragEvent| e.prevent_default());

    // ----------------- Connection logic -----------------
    let on_connection_start = {
        let pending_connection = pending_connection.clone();
        let pending_line = pending_line.clone();
        let blocks = blocks.clone();
        Callback::from(move |from_id: usize| {
            pending_connection.set(Some(from_id));
            if let Some(block) = (*blocks).iter().find(|b| b.id == from_id) {
                let x = block.position.0 + 100.0;
                let y = block.position.1 + 25.0;
                pending_line.set(Some(((x, y), (x, y))));
            }
        })
    };

    let on_connection_drop = {
        let pending_connection = pending_connection.clone();
        let connections = connections.clone();
        let pending_line = pending_line.clone();
        let blocks = blocks.clone();
        Callback::from(move |to_id: usize| {
            if let Some(from_id) = *pending_connection {
                if from_id != to_id {
                    let current_blocks = (*blocks).clone();
                    if let (Some(from_block), Some(to_block)) = (
                        current_blocks.iter().find(|b| b.id == from_id),
                        current_blocks.iter().find(|b| b.id == to_id),
                    ) {
                        // ✅ Check connection rules before adding
                        if can_connect(from_block, to_block, &connections, &blocks) {
                            let mut current_connections = (*connections).clone();
                            current_connections.push(Connection { from: from_id, to: to_id });
                            connections.set(current_connections);
                        } else {
                            // ❌ (Optional) visual feedback
                            web_sys::console::log_1(
                                &format!(
                                    "Connection from {:?} to {:?} not allowed",
                                    from_block.block_type, to_block.block_type
                                )
                                    .into(),
                            );
                        }
                    }
                }
            }

            // Snap pending line end to target port
            if let Some(to_block) = (*blocks).iter().find(|b| b.id == to_id) {
                let x = to_block.position.0;
                let y = to_block.position.1 + 25.0;
                if let Some(((from_x, from_y), _)) = *pending_line {
                    pending_line.set(Some(((from_x, from_y), (x, y))));
                }
            }

            pending_connection.set(None);
            pending_line.set(None);
        })
    };

    // ----------------- Drag block logic -----------------
    let on_block_mouse_down = {
        let dragging_block = dragging_block.clone();
        let drag_offset = drag_offset.clone();
        let canvas_ref = canvas_ref.clone();
        let blocks = blocks.clone();
        Callback::from(move |(block_id, e): (usize, MouseEvent)| {
            e.prevent_default();
            if let Some(canvas) = canvas_ref.cast::<HtmlElement>() {
                let rect = canvas.get_bounding_client_rect();
                let mouse_x = e.client_x() as f32 - rect.left() as f32;
                let mouse_y = e.client_y() as f32 - rect.top() as f32;
                if let Some(block) = (*blocks).iter().find(|b| b.id == block_id) {
                    drag_offset.set((mouse_x - block.position.0, mouse_y - block.position.1));
                    dragging_block.set(Some(block_id));
                }
            }
        })
    };

    // ----------------- Mouse move for both pending line and block drag -----------------
    let on_canvas_mouse_move = {
        let pending_line = pending_line.clone();
        let dragging_block = dragging_block.clone();
        let drag_offset = drag_offset.clone();
        let blocks = blocks.clone();
        let canvas_ref = canvas_ref.clone();
        Callback::from(move |e: MouseEvent| {
            if let Some(canvas) = canvas_ref.cast::<HtmlElement>() {
                let rect = canvas.get_bounding_client_rect();
                let mouse_x = e.client_x() as f32 - rect.left() as f32;
                let mouse_y = e.client_y() as f32 - rect.top() as f32;

                // 1️⃣ Update pending line (snap to nearest port if close)
                if let Some(((from_x, from_y), _)) = *pending_line {
                    let mut snapped = (mouse_x, mouse_y);
                    for block in (*blocks).iter() {
                        let port_x = block.position.0;
                        let port_y = block.position.1 + 25.0;
                        let dx = mouse_x - port_x;
                        let dy = mouse_y - port_y;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist < 10.0 { // Snap distance threshold
                            snapped = (port_x, port_y);
                        }
                    }
                    pending_line.set(Some(((from_x, from_y), snapped)));
                }

                // 2️⃣ Update dragging block
                if let Some(block_id) = *dragging_block {
                    let (offset_x, offset_y) = *drag_offset;
                    let mut current_blocks = (*blocks).clone();
                    if let Some(block) = current_blocks.iter_mut().find(|b| b.id == block_id) {
                        block.position = (mouse_x - offset_x, mouse_y - offset_y);
                    }
                    blocks.set(current_blocks);
                }
            }
        })
    };

    let on_canvas_mouse_up = {
        let dragging_block = dragging_block.clone();
        Callback::from(move |_e: MouseEvent| {
            dragging_block.set(None);
        })
    };

    let on_canvas_right_click = {
        let pending_connection = pending_connection.clone();
        let pending_line = pending_line.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default(); // prevent default browser context menu
            pending_connection.set(None);
            pending_line.set(None);
        })
    };

    // ----------------- Delete handlers -----------------
    let toggle_delete_mode = {
        let delete_mode = delete_mode.clone();
        Callback::from(move |_| delete_mode.set(!*delete_mode))
    };

    let delete_block = {
        let blocks = blocks.clone();
        let connections = connections.clone();
        Callback::from(move |block_id: usize| {
            let mut current_blocks = (*blocks).clone();
            current_blocks.retain(|b| b.id != block_id);
            blocks.set(current_blocks);

            let mut current_connections = (*connections).clone();
            current_connections.retain(|c| c.from != block_id && c.to != block_id);
            connections.set(current_connections);
        })
    };

    let delete_connection = {
        let connections = connections.clone();
        Callback::from(move |(from, to): (usize, usize)| {
            let mut current_connections = (*connections).clone();
            current_connections.retain(|c| !(c.from == from && c.to == to));
            connections.set(current_connections);
        })
    };


    // ----------------- Render -----------------
    html! {
        <div class="tp__source-editor">

            <SourceEditorSidebar
                delete_mode={*delete_mode}
                on_drag_start={on_drag_start.clone()}
                on_toggle_delete={toggle_delete_mode.clone()}
            />
            // Canvas
            <div
                ref={canvas_ref.clone()}
                class="tp__source-editor__canvas graph-paper-advanced"
                ondrop={on_drop.clone()}
                ondragover={on_drag_over.clone()}
                onmousemove={on_canvas_mouse_move}
                onmouseup={on_canvas_mouse_up}
                oncontextmenu={on_canvas_right_click}>

                // SVG for connections
                <svg class="tp__source-editor__connections">
                    { for (*connections).iter().filter_map(|c| {
                        let from_block = (*blocks).iter().find(|b| b.id == c.from)?;
                        let to_block = (*blocks).iter().find(|b| b.id == c.to)?;
                        let from_x = from_block.position.0 + 100.0;
                        let from_y = from_block.position.1 + 25.0;
                        let to_x = to_block.position.0;
                        let to_y = to_block.position.1 + 25.0;
                        let dx = to_x - from_x;
                        let ctrl = dx * 0.5;
                        let d = format!(
                            "M {} {} C {} {}, {} {}, {} {}",
                            from_x, from_y,
                            from_x + ctrl, from_y,
                            to_x - ctrl, to_y,
                            to_x, to_y
                        );

                        Some(html! {
                            <g>
                                <path d={d} stroke="white" fill="transparent" stroke-width="2"/>
                                { if *delete_mode {
                                    let mid_x = (from_x + to_x) / 2.0;
                                    let mid_y = (from_y + to_y) / 2.0;
                                    let on_delete_connection = delete_connection.clone();
                                    html! {
                                        <circle cx={mid_x.to_string()} cy={mid_y.to_string()} r="6" fill="var(--source-editor-delete-color)" class="clickable"
                                            onclick={
                                                let from = c.from;
                                                let to = c.to;
                                                Callback::from(move |_| on_delete_connection.emit((from, to)))
                                            }
                                        />
                                    }
                                } else {
                                    html!{}
                                } }
                            </g>
                        })
                    }) }

                    // Pending line (straight, yellow)
                    { if let Some(((x1, y1), (x2, y2))) = *pending_line {
                        html! {
                            <line
                                x1={x1.to_string()} y1={y1.to_string()}
                                x2={x2.to_string()} y2={y2.to_string()}
                                stroke="yellow"
                                stroke-width="2"
                                stroke-dasharray="4 2" />
                        }
                    } else { html!{} } }
                </svg>

                // Render blocks
                { for (*blocks).iter().map(|b| {
                    let block_id = b.id;
                    let style = format!("position:absolute; left:{}px; top:{}px;", b.position.0, b.position.1);
                    let from_id = block_id;
                    let to_id = block_id;
                    let delete_mode = *delete_mode;
                    let delete_block = delete_block.clone();

                    let is_target = matches!(b.block_type, BlockType::Target);
                    let is_input = !is_target && matches!(b.block_type, BlockType::InputM3u | BlockType::InputXtream);
                    let is_output =  !is_input && !is_target;

                    let port_status = if let Some(from_id) = *pending_connection {
                        if let Some(from_block) = (*blocks).iter().find(|b| b.id == from_id) {
                            if can_connect(from_block, b, &connections, &blocks) {
                                PORT_VALID
                            } else {
                                PORT_INVALID
                            }
                        } else {
                            PORT_INACTIVE
                        }
                    } else {
                        PORT_INACTIVE
                    };

                    let port_style = match port_status {
                        PORT_VALID =>  "tp__source-editor__port--valid",
                        PORT_INVALID =>  "tp__source-editor__port--invalid",
                        _ => "",
                    };

                    html! {
                        <div class={format!("tp__source-editor__block no-select tp__source-editor__brick-{}", b.block_type)}
                            style={style}>
                            // Block handle (drag)
                            <div
                                class="tp__source-editor__block-handle"
                                onmousedown={{
                                    let on_block_mouse_down = on_block_mouse_down.clone();
                                    let block_id = b.id;
                                    Callback::from(move |e| on_block_mouse_down.emit((block_id, e)))
                                }}>
                            </div>

                            // Delete button for block
                            {
                                html_if!(delete_mode, {
                                    <div class="tp__source-editor__block-delete" onclick={
                                        let block_id = b.id;
                                        Callback::from(move |_| delete_block.emit(block_id))
                                    }></div>
                                })
                            }

                           {html_if!(is_target || is_output, {
                            // Left port
                            <div
                                class={classes!("tp__source-editor__port", "tp__source-editor__port--left", port_style)}
                                onmouseup={{
                                    let on_connection_drop = on_connection_drop.clone();
                                    Callback::from(move |_| on_connection_drop.emit(to_id))
                                }} />
                            })}
                            // Block label
                            <div class="tp__source-editor__block-label">
                                { translate.t(&format!("SOURCE_EDITOR.BRICK_{}", b.block_type)) }
                            </div>

                           {html_if!(is_target || is_input, {
                            // Right port
                            <div
                                class="tp__source-editor__port tp__source-editor__port--right"
                                onmousedown={{
                                    let on_connection_start = on_connection_start.clone();
                                    Callback::from(move |_| on_connection_start.emit(from_id))
                                }} />
                            })}
                        </div>
                    }
                }) }
            </div>
        </div>
    }
}
