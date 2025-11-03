use std::rc::Rc;
use crate::app::components::source_editor::editor_model::{Block, BlockType, Connection};
use crate::app::components::source_editor::rules::can_connect;
use crate::app::components::source_editor::sidebar::SourceEditorSidebar;
use web_sys::{HtmlElement, MouseEvent};
use yew::prelude::*;
use shared::model::{ConfigInputDto, ConfigTargetDto, HdHomeRunTargetOutputDto, M3uTargetOutputDto, StrmTargetOutputDto, TargetOutputDto, XtreamTargetOutputDto};
use crate::app::components::{BlockId, BlockInstance, EditMode, PortStatus, SourceEditorContext, SourceEditorForm};
use crate::app::components::source_editor::block::BlockView;

const BLOCK_WIDTH: f32 = 100.0;
const BLOCK_HEIGHT: f32 = 50.0;
const BLOCK_HEADER_HEIGHT: f32 = 12.0;
const BLOCK_PORT_HEIGHT: f32 = 10.0;
const BLOCK_MIDDLE_Y: f32 = (BLOCK_HEIGHT + BLOCK_HEADER_HEIGHT + BLOCK_PORT_HEIGHT)  / 2.0;

fn create_instance(block_type: BlockType) -> BlockInstance {
    match block_type {
        BlockType::InputXtream => BlockInstance::Input(Rc::new(ConfigInputDto::default())),
        BlockType::InputM3u => BlockInstance::Input(Rc::new(ConfigInputDto::default())),
        BlockType::Target => {
            let mut dto = ConfigTargetDto::default();
            dto.name = String::new();
            BlockInstance::Target(Rc::new(dto))
        },
        BlockType::OutputM3u => BlockInstance::Output(Rc::new(TargetOutputDto::M3u(M3uTargetOutputDto::default()))),
        BlockType::OutputXtream => BlockInstance::Output(Rc::new(TargetOutputDto::Xtream(XtreamTargetOutputDto::default()))),
        BlockType::OutputHdHomeRun => BlockInstance::Output(Rc::new(TargetOutputDto::HdHomeRun(HdHomeRunTargetOutputDto::default()))),
        BlockType::OutputStrm => BlockInstance::Output(Rc::new(TargetOutputDto::Strm(StrmTargetOutputDto::default()))),
    }
}

// ----------------- Component -----------------
#[function_component]
pub fn SourceEditor() -> Html {
    let canvas_ref = use_node_ref();
    let blocks = use_state(Vec::<Block>::new);
    let connections = use_state(Vec::<Connection>::new);
    let next_id = use_state(|| 1usize);

    // Dragging state
    let dragging_block = use_state(|| None);
    let drag_offset = use_state(|| (0.0f32, 0.0f32));
    let sidebar_drag_offset = use_state(|| (0.0f32, 0.0f32));

    // Pending line for live connection
    let pending_line = use_state(|| None);
    let pending_connection = use_state(|| None );

    // Delete mode toggle
    let delete_mode = use_state(|| false);

    // ----------------- Drag Start from Sidebar -----------------
    let handle_drag_start = {
        let sidebar_drag_offset = sidebar_drag_offset.clone();
        Callback::from(move |e: DragEvent| {
            if let Some(target) = e.target_dyn_into::<HtmlElement>() {
                let block_type = target.get_attribute("data-block-type").unwrap_or_default();
                e.data_transfer().unwrap().set_data("text/plain", &block_type).unwrap();
                // Store mouse offset inside the element
                let rect = target.get_bounding_client_rect();
                let offset_x = e.client_x() as f32 - rect.left() as f32;
                let offset_y = e.client_y() as f32 - rect.top() as f32;
                sidebar_drag_offset.set((offset_x, offset_y));
            }
        }
    )};

    // ----------------- Drop on Canvas -----------------
    let handle_drop = {
        let blocks = blocks.clone();
        let next_id = next_id.clone();
        let canvas_ref = canvas_ref.clone();
        let sidebar_drag_offset = sidebar_drag_offset.clone();
        Callback::from(move |e: DragEvent| {
            e.prevent_default();
            if let Some(canvas) = canvas_ref.cast::<HtmlElement>() {
                if let Ok(data) = e.data_transfer().unwrap().get_data("text/plain") {
                    let rect = canvas.get_bounding_client_rect();
                    let mouse_x = e.client_x() as f32 - rect.left() as f32;
                    let mouse_y = e.client_y() as f32 - rect.top() as f32;
                    let (offset_x, offset_y) = *sidebar_drag_offset;

                    let block_type = BlockType::from(data.as_str());

                    let mut current_blocks = (*blocks).clone();
                    current_blocks.push(Block {
                        id: *next_id,
                        block_type,
                        position: (mouse_x - offset_x, mouse_y - offset_y),
                        instance: create_instance(block_type),
                    });
                    blocks.set(current_blocks);
                    next_id.set(*next_id + 1);
                }
            }
        })
    };
    let handle_drag_over = Callback::from(|e: DragEvent| e.prevent_default());

    // ----------------- Connection logic -----------------
    let handle_connection_start = {
        let pending_connection = pending_connection.clone();
        let pending_line = pending_line.clone();
        let blocks = blocks.clone();
        Callback::from(move |from_id: usize| {
            pending_connection.set(Some(from_id));
            if let Some(block) = (*blocks).iter().find(|b| b.id == from_id) {
                let x = block.position.0 + BLOCK_WIDTH;
                let y = block.position.1 + BLOCK_MIDDLE_Y;
                pending_line.set(Some(((x, y), (x, y))));
            }
        })
    };

    let handle_connection_drop = {
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
                let y = to_block.position.1 + BLOCK_MIDDLE_Y;
                if let Some(((from_x, from_y), _)) = *pending_line {
                    pending_line.set(Some(((from_x, from_y), (x, y))));
                }
            }

            pending_connection.set(None);
            pending_line.set(None);
        })
    };

    // ----------------- Drag block logic -----------------
    let handle_block_mouse_down = {
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
    let handle_canvas_mouse_move = {
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

                // Update pending line (snap to nearest port if close)
                if let Some(((from_x, from_y), _)) = *pending_line {
                    let mut snapped = (mouse_x, mouse_y);
                    for block in (*blocks).iter() {
                        let port_x = block.position.0;
                        let port_y = block.position.1 + BLOCK_MIDDLE_Y;
                        let dx = mouse_x - port_x;
                        let dy = mouse_y - port_y;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist < 10.0 { // Snap distance threshold
                            snapped = (port_x, port_y);
                        }
                    }
                    pending_line.set(Some(((from_x, from_y), snapped)));
                }

                // Update dragging block
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

    let handle_canvas_mouse_up = {
        let dragging_block = dragging_block.clone();
        Callback::from(move |_e: MouseEvent| {
            dragging_block.set(None);
        })
    };

    let handle_canvas_right_click = {
        let pending_connection = pending_connection.clone();
        let pending_line = pending_line.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default(); // prevent default browser context menu
            pending_connection.set(None);
            pending_line.set(None);
        })
    };

    // ----------------- Delete handlers -----------------
    let handle_toggle_delete_mode = {
        let delete_mode = delete_mode.clone();
        Callback::from(move |_| delete_mode.set(!*delete_mode))
    };

    let handle_delete_block = {
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

    let handle_delete_connection = {
        let connections = connections.clone();
        Callback::from(move |(from, to): (usize, usize)| {
            let mut current_connections = (*connections).clone();
            current_connections.retain(|c| !(c.from == from && c.to == to));
            connections.set(current_connections);
        })
    };

    let get_port_status = {
        |block: &Block| {
            if let Some(from_id) = *pending_connection {
                if let Some(from_block) = (*blocks).iter().find(|b| b.id == from_id) {
                    return if can_connect(from_block, block, &connections, &blocks) {
                        PortStatus::Valid
                    } else {
                        PortStatus::Invalid
                    }
                }
            }
            PortStatus::Inactive
        }
    };

    let form_changed = {
        let blocks = blocks.clone();
        Callback::<(BlockId, BlockInstance)>::from(move |(block_id, instance): (BlockId, BlockInstance)| {
            if let Some(block) = (*blocks).iter().find(|b| b.id == block_id) {
                let mut current_blocks = (*blocks).clone();
                current_blocks.retain(|b| b.id != block_id);
                let mut new_block = block.clone();
                new_block.instance = instance;
                current_blocks.push(new_block);
                blocks.set(current_blocks);
            }
        })
    };

    let edit_mode = use_state(|| EditMode::Inactive);

    let handle_block_edit = {
        let edit_mode_set = edit_mode.clone();
        let blocks = blocks.clone();
        Callback::from(move |block_id: usize| {
            if let Some(block) = (*blocks).iter().find(|b| b.id == block_id) {
                edit_mode_set.set(EditMode::Active(block.clone()));
            }
        })
    };

    let editor_context = SourceEditorContext {
        on_form_change: form_changed,
        edit_mode: edit_mode.clone(),
    };

    // ----------------- Render -----------------
    html! {
        <ContextProvider<SourceEditorContext> context={editor_context}>
        <span>{"WORK IN PROGRESS - NOT FINALIZED !!!"}</span>
        <div class="tp__source-editor">

            <SourceEditorSidebar
                delete_mode={*delete_mode}
                on_drag_start={handle_drag_start.clone()}
                on_toggle_delete={handle_toggle_delete_mode.clone()}
            />
            // Canvas
            <div class="tp__source-editor__canvas-wrapper">
            <div
                ref={canvas_ref.clone()}
                class="tp__source-editor__canvas graph-paper-advanced"
                ondrop={handle_drop.clone()}
                ondragover={handle_drag_over.clone()}
                onmousemove={handle_canvas_mouse_move}
                onmouseup={handle_canvas_mouse_up}
                oncontextmenu={handle_canvas_right_click}>

                // SVG for connections
                <svg class="tp__source-editor__connections">
                    { for (*connections).iter().filter_map(|c| {
                        let from_block = (*blocks).iter().find(|b| b.id == c.from)?;
                        let to_block = (*blocks).iter().find(|b| b.id == c.to)?;
                        let from_x = from_block.position.0 + BLOCK_WIDTH;
                        let from_y = from_block.position.1 + BLOCK_MIDDLE_Y;
                        let to_x = to_block.position.0;
                        let to_y = to_block.position.1 + BLOCK_MIDDLE_Y;
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
                                    let on_delete_connection = handle_delete_connection.clone();
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
                { for (*blocks).iter().map(|b|{
                    let port_status = get_port_status(b);
                    html! {
                    <BlockView
                        block={b.clone()}
                        delete_mode={*delete_mode}
                        delete_block={handle_delete_block.clone()}
                        port_status={port_status}
                        on_edit={handle_block_edit.clone()}
                        on_mouse_down={handle_block_mouse_down.clone()}
                        on_connection_drop={handle_connection_drop.clone()}
                        on_connection_start={handle_connection_start.clone()}
                    />
                }}) }
            </div>
            </div>
            <SourceEditorForm />
        </div>
        </ContextProvider<SourceEditorContext>>
    }
}
