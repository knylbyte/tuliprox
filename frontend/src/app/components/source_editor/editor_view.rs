use crate::app::components::source_editor::layout::layout;
use crate::app::components::{can_connect, Block, BlockId, BlockInstance, BlockType, BlockView, Connection, EditMode, InputRow, PortStatus, SourceEditorContext, SourceEditorForm, SourceEditorSidebar, BLOCK_HEADER_HEIGHT, BLOCK_HEIGHT, BLOCK_PORT_HEIGHT, BLOCK_WIDTH};
use crate::app::PlaylistContext;
use shared::model::{ConfigInputDto, ConfigTargetDto, HdHomeRunTargetOutputDto, M3uTargetOutputDto, StrmTargetOutputDto, TargetOutputDto, XtreamTargetOutputDto};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, MouseEvent};
use yew::prelude::*;

const BLOCK_MIDDLE_Y: f32 = (BLOCK_HEIGHT + BLOCK_HEADER_HEIGHT + BLOCK_PORT_HEIGHT) / 2.0;

#[derive(Clone, PartialEq)]
struct DragState {
    pub block_id: Option<BlockId>,
    pub drag_offset: (f32, f32),
    pub sidebar_drag_offset: (f32, f32),
    pub dragging_group: Vec::<BlockId>,
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            block_id: None,
            drag_offset: (0.0f32, 0.0f32),
            dragging_group: Vec::<BlockId>::new(),
            sidebar_drag_offset: (0.0f32, 0.0f32),
        }
    }
}

impl DragState {
    pub fn with_sidebar_drag_offset(&self, x: f32, y: f32) -> Self {
        Self { sidebar_drag_offset: (x, y), ..self.clone() }
    }

    pub fn with_drag_block_offset_and_group(&self, block_id: BlockId, drag_offset: (f32, f32), dragging_group: Vec<BlockId>) -> Self {
        Self { block_id: Some(block_id), drag_offset, dragging_group, sidebar_drag_offset: self.sidebar_drag_offset }
    }

    pub(crate) fn reset_dragging(&self) -> DragState {
        Self::default()
    }
}

#[derive(Clone, PartialEq)]
struct SelectionState {
    pub is_selecting: bool,
    pub selection_rect: Option<(f32, f32, f32, f32)>, // x,y,w,h relative to canva;
    pub selection_start: (f32, f32),
    pub selected_blocks: Vec::<BlockId>,
    pub group_initial_positions: Vec::<(BlockId, (f32, f32))>,
    pub group_anchor_mouse: (f32, f32),
}

impl Default for SelectionState {
    fn default() -> Self {
        Self {
            is_selecting: false,
            selection_rect: None,
            selection_start: (0.0f32, 0.0f32),
            selected_blocks: Vec::<BlockId>::new(),
            group_initial_positions: Vec::<(BlockId, (f32, f32))>::new(),
            group_anchor_mouse: (0.0f32, 0.0f32),
        }
    }
}

impl SelectionState {
    pub fn reset_selection(&self) -> Self {
        Self::default()
    }
    pub fn with_blocks_group_initial_pos_and_group_anchor_mouse(&self, selected_blocks: Vec<BlockId>,
                                                                group_initial_positions: Vec<(BlockId, (f32, f32))>,
                                                                group_anchor_mouse: (f32, f32)) -> Self {
        Self {
            is_selecting: self.is_selecting,
            selection_rect: self.selection_rect,
            selection_start: self.selection_start,
            selected_blocks,
            group_initial_positions,
            group_anchor_mouse,
        }
    }

    pub fn with_selecting_start_rect_and_blocks(&self, is_selecting: bool,
                                                selection_start: (f32, f32),
                                                selection_rect: Option<(f32, f32, f32, f32)>,
                                                selected_blocks: Vec::<BlockId>) -> Self {
        Self {
            is_selecting,
            selection_rect,
            selection_start,
            selected_blocks,
            group_initial_positions: self.group_initial_positions.clone(),
            group_anchor_mouse: self.group_anchor_mouse,
        }
    }

    pub fn with_selecting_start_and_rect(&self, is_selecting: bool,
                                         selection_start: (f32, f32),
                                         selection_rect: Option<(f32, f32, f32, f32)>) -> Self {
        Self {
            is_selecting,
            selection_rect,
            selection_start,
            selected_blocks: self.selected_blocks.clone(),
            group_initial_positions: self.group_initial_positions.clone(),
            group_anchor_mouse: self.group_anchor_mouse,
        }
    }


    pub fn with_rect_and_blocks(&self, selection_rect: Option<(f32, f32, f32, f32)>,
                                selected_blocks: Vec::<BlockId>) -> Self {
        Self {
            is_selecting: self.is_selecting,
            selection_rect,
            selection_start: self.selection_start,
            selected_blocks,
            group_initial_positions: self.group_initial_positions.clone(),
            group_anchor_mouse: self.group_anchor_mouse,
        }
    }

    pub fn with_selecting_rect_and_group_initial_positions(&self, is_selecting: bool,
                                                           selection_rect: Option<(f32, f32, f32, f32)>,
                                                           group_initial_positions: Vec<(BlockId, (f32, f32))>) -> Self {
        Self {
            is_selecting,
            selection_rect,
            selection_start: self.selection_start,
            selected_blocks: self.selected_blocks.clone(),
            group_initial_positions,
            group_anchor_mouse: self.group_anchor_mouse,
        }
    }

    pub(crate) fn with_cleared_blocks(&self, block_id: BlockId) -> Self {
        let updated_selections: Vec<BlockId> = self.selected_blocks
            .iter()
            .filter(|&&id| id != block_id)
            .map(|&id| if id > block_id { id - 1 } else { id })
            .collect();

        Self {
            is_selecting: self.is_selecting,
            selection_rect: self.selection_rect,
            selection_start: self.selection_start,
            selected_blocks: updated_selections,
            group_initial_positions: self.group_initial_positions.clone(),
            group_anchor_mouse: self.group_anchor_mouse,
        }
    }
}

fn create_instance(block_type: BlockType) -> BlockInstance {
    match block_type {
        BlockType::InputXtream => BlockInstance::Input(Rc::new(ConfigInputDto::default())),
        BlockType::InputM3u => BlockInstance::Input(Rc::new(ConfigInputDto::default())),
        BlockType::Target => {
            let dto = ConfigTargetDto { name: String::new(), ..Default::default() };
            BlockInstance::Target(Rc::new(dto))
        }
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
    let playlist_ctx = use_context::<PlaylistContext>().expect("Playlist context not found");
    let force_update = use_state(|| 0);
    let blocks = use_mut_ref(Vec::<Block>::new);
    let connections = use_mut_ref(Vec::<Connection>::new);
    let next_id = use_state(|| 1 as BlockId);
    let block_elements = use_mut_ref(HashMap::<BlockId, HtmlElement>::new);
    let connection_elements = use_mut_ref(HashMap::<(BlockId, BlockId), Element>::new);
    // ----------------- virtual canvas offset -----------------
    let canvas_offset = use_state(|| (0.0f32, 0.0f32));
    let is_panning = use_state(|| false);
    let pan_start = use_state(|| (0.0f32, 0.0f32));
    // Dragging state
    let drag_state = use_state(DragState::default);
    // Selection / marquee states
    let selection_state = use_state(SelectionState::default);
    // Pending line for live connection
    let pending_line = use_state(|| None);
    let pending_connection = use_state(|| None);
    // Delete mode toggle
    let delete_mode = use_state(|| false);
    let cursor_grabbing = use_state(|| false);

    {
        let playlists = playlist_ctx.clone();
        let get_next_id = next_id.clone();
        let blocks_ref = blocks.clone();
        let connections_ref = connections.clone();
        use_effect_with(playlists.sources.clone(), move |sources| {
            if let Some(entries) = sources.as_ref() {
                let mut gen_blocks = blocks_ref.borrow_mut();
                let mut gen_connections = connections_ref.borrow_mut();
                let mut current_id = *get_next_id;
                for (inputs, targets) in entries.as_ref() {
                    let mut input_ids = vec![];
                    for input_row in inputs {
                        match input_row.as_ref() {
                            InputRow::Input(input_config) => {
                                let input_id = current_id;
                                current_id += 1;
                                let block = Block {
                                    id: input_id,
                                    block_type: BlockType::from(input_config.input_type),
                                    position: (0.0, 0.0),
                                    instance: BlockInstance::Input(input_config.clone()),
                                };
                                input_ids.push(input_id);
                                gen_blocks.push(block);
                            }
                            InputRow::Alias(_, _) => {}
                        }
                    }
                    for target_config in targets {
                        let target_id = current_id;
                        current_id += 1;
                        let block = Block {
                            id: target_id,
                            block_type: BlockType::Target,
                            position: (0.0, 0.0),
                            instance: BlockInstance::Target(target_config.clone()),
                        };
                        gen_blocks.push(block);
                        input_ids.iter().for_each(|input_id| gen_connections.push(Connection { from: *input_id, to: target_id }));

                        for output in &target_config.output {
                            let (block_instance, block_type) = match output {
                                TargetOutputDto::Xtream(dto) => (BlockInstance::Output(Rc::new(TargetOutputDto::Xtream(dto.clone()))), BlockType::OutputXtream),
                                TargetOutputDto::M3u(dto) => (BlockInstance::Output(Rc::new(TargetOutputDto::M3u(dto.clone()))), BlockType::OutputM3u),
                                TargetOutputDto::Strm(dto) => (BlockInstance::Output(Rc::new(TargetOutputDto::Strm(dto.clone()))), BlockType::OutputStrm),
                                TargetOutputDto::HdHomeRun(dto) => (BlockInstance::Output(Rc::new(TargetOutputDto::HdHomeRun(dto.clone()))), BlockType::OutputHdHomeRun),
                            };
                            let output_id = current_id;
                            current_id += 1;

                            let block = Block {
                                id: output_id,
                                block_type,
                                position: (0.0, 0.0),
                                instance: block_instance,
                            };
                            gen_blocks.push(block);
                            gen_connections.push(Connection { from: target_id, to: output_id });
                        }
                    }
                }
                layout(&mut gen_blocks, &gen_connections);
                get_next_id.set(current_id);
            }

            || {}
        });
    }


    let handle_layout = {
        let blocks_clone = blocks.clone();
        let connections_clone = connections.clone();
        let force_update = force_update.clone();
        Callback::from(move |_| {
            let mut new_blocks = blocks_clone.borrow_mut();
            let new_connections = connections_clone.borrow_mut();
            layout(&mut new_blocks, &new_connections);

            force_update.set(*force_update + 1);
        })
    };

    // ----------------- Drag Start from Sidebar -----------------
    let handle_drag_start = {
        let drag_state = drag_state.clone();
        let cursor_grabbing = cursor_grabbing.clone();
        let selection_state = selection_state.clone();
        Callback::from(move |e: DragEvent| {
            selection_state.set((*selection_state).reset_selection());
            if let Some(target) = e.target_dyn_into::<HtmlElement>() {
                let block_type = target.get_attribute("data-block-type").unwrap_or_default();
                e.data_transfer().unwrap().set_data("text/plain", &block_type).unwrap();
                // Store mouse offset inside the element
                let rect = target.get_bounding_client_rect();
                let offset_x = e.client_x() as f32 - rect.left() as f32;
                let offset_y = e.client_y() as f32 - rect.top() as f32;
                drag_state.set((*drag_state).with_sidebar_drag_offset(offset_x, offset_y));
                cursor_grabbing.set(true);
            }
        }
        )
    };

    // ----------------- Drop on Canvas -----------------
    let handle_drop = {
        let blocks = blocks.clone();
        let next_id = next_id.clone();
        let canvas_ref = canvas_ref.clone();
        let drag_state = drag_state.clone();
        let canvas_offset = canvas_offset.clone(); // <-- add canvas_offset
        let cursor_grabbing = cursor_grabbing.clone();

        Callback::from(move |e: DragEvent| {
            e.prevent_default();
            cursor_grabbing.set(false);
            if let Some(canvas) = canvas_ref.cast::<HtmlElement>() {
                if let Ok(data) = e.data_transfer().unwrap().get_data("text/plain") {
                    let rect = canvas.get_bounding_client_rect();
                    let mouse_x = e.client_x() as f32 - rect.left() as f32;
                    let mouse_y = e.client_y() as f32 - rect.top() as f32;
                    let (offset_x, offset_y) = drag_state.sidebar_drag_offset;
                    let (ox, oy) = *canvas_offset; // <-- include canvas offset

                    let block_type = BlockType::from(data.as_str());

                    let mut current_blocks = blocks.borrow_mut();
                    current_blocks.push(Block {
                        id: *next_id,
                        block_type,
                        position: (
                            mouse_x - offset_x - ox, // <-- subtract canvas offset
                            mouse_y - offset_y - oy
                        ),
                        instance: create_instance(block_type),
                    });
                    next_id.set(*next_id + 1);
                }
            }
        })
    };

    let handle_drag_over = Callback::from(|e: DragEvent| e.prevent_default());
    let handle_drag_end = {
        let cursor_grabbing = cursor_grabbing.clone();
        Callback::from(move |e: DragEvent| {
            cursor_grabbing.set(false);
            e.prevent_default()
        })
    };

    // ----------------- Connection logic -----------------
    let handle_connection_start = {
        let pending_connection = pending_connection.clone();
        let pending_line = pending_line.clone();
        let blocks = blocks.clone();
        let canvas_offset = canvas_offset.clone();
        Callback::from(move |from_id: BlockId| {
            pending_connection.set(Some(from_id));
            if let Some(block) = blocks.borrow().get(from_id as usize - 1) {
                let (ox, oy) = *canvas_offset;
                let x = block.position.0 + BLOCK_WIDTH + ox;
                let y = block.position.1 + BLOCK_MIDDLE_Y + oy;
                pending_line.set(Some(((x, y), (x, y))));
            }
        })
    };

    let handle_connection_drop = {
        let pending_connection = pending_connection.clone();
        let connections = connections.clone();
        let pending_line = pending_line.clone();
        let blocks = blocks.clone();
        Callback::from(move |to_id: BlockId| {
            if let Some(from_id) = *pending_connection {
                if from_id != to_id {
                    let current_blocks = blocks.borrow_mut();
                    if let (Some(from_block), Some(to_block)) = (
                        current_blocks.get(from_id as usize - 1),
                        current_blocks.get(to_id as usize - 1),
                    ) {
                        // Check connection rules before adding
                        if can_connect(from_block, to_block, &connections.borrow(), &current_blocks) {
                            let mut current_connections = connections.borrow_mut();
                            current_connections.push(Connection { from: from_id, to: to_id });
                        }
                    }
                }
            }

            pending_connection.set(None);
            pending_line.set(None);
        })
    };

    // ----------------- Drag block logic  -----------------
    let handle_block_mouse_down = {
        let drag_state = drag_state.clone();
        let canvas_ref = canvas_ref.clone();
        let blocks = blocks.clone();
        let cursor_grabbing = cursor_grabbing.clone();
        let selection_state = selection_state.clone();

        Callback::from(move |(block_id, e): (BlockId, MouseEvent)| {
            e.prevent_default();
            let ctrl_key = e.ctrl_key();
            if let Some(canvas) = canvas_ref.cast::<HtmlElement>() {
                cursor_grabbing.set(true);
                let rect = canvas.get_bounding_client_rect();
                let mouse_x = e.client_x() as f32 - rect.left() as f32;
                let mouse_y = e.client_y() as f32 - rect.top() as f32;
                if let Some(block) = blocks.borrow().get(block_id as usize - 1) {
                    // For Group-Drag: save all blocks
                    let mut initials = vec![];
                    let mut group_ids = vec![];

                    // if block is not in selection list, than only select this block
                    let mut new_selection = None;
                    if !selection_state.selected_blocks.contains(&block_id) {
                        new_selection = Some(block_id);
                    } else {
                        let current_blocks = blocks.borrow();
                        for id in &selection_state.selected_blocks {
                            if let Some(b) = current_blocks.get(*id as usize - 1) {
                                initials.push((*id, b.position));
                                group_ids.push(*id);
                            }
                        }
                    }

                    drag_state.set((*drag_state).with_drag_block_offset_and_group(block_id, (mouse_x - block.position.0, mouse_y - block.position.1), group_ids));

                    let new_blocks = match new_selection {
                        Some(block) => if ctrl_key {
                            let mut new_list = selection_state.selected_blocks.clone();
                            new_list.push(block);
                            new_list
                        } else { vec![block] },
                        None => selection_state.selected_blocks.clone(),
                    };
                    selection_state.set((*selection_state).with_blocks_group_initial_pos_and_group_anchor_mouse(new_blocks, initials, (mouse_x, mouse_y)));
                }
            }
        })
    };

    // ----------------- Canvas mouse down (start panning or marquee selection) -----------------
    let handle_canvas_mouse_down = {
        let is_panning = is_panning.clone();
        let selection_state = selection_state.clone();
        let pan_start = pan_start.clone();
        let canvas_ref = canvas_ref.clone();
        let cursor_grabbing = cursor_grabbing.clone();

        Callback::from(move |e: MouseEvent| {
            let mouse_button = e.button();
            if mouse_button != 0 && mouse_button != 2 {
                return;
            }
            if let Some(target) = e.target_dyn_into::<web_sys::Element>() {
                if let Some(canvas) = canvas_ref.cast::<web_sys::Element>() {
                    let tag = target.tag_name().to_lowercase();
                    if target.is_same_node(Some(&canvas)) || tag == "svg" {
                        e.prevent_default();

                        if e.button() == 0 { // left button
                            if selection_state.is_selecting {
                                selection_state.set((*selection_state).reset_selection());
                            } else {
                                // selection area mode
                                if let Some(rect_el) = canvas_ref.cast::<HtmlElement>() {
                                    let rect = rect_el.get_bounding_client_rect();
                                    let mouse_x = e.client_x() as f32 - rect.left() as f32;
                                    let mouse_y = e.client_y() as f32 - rect.top() as f32;
                                    if e.ctrl_key() {
                                        selection_state.set((*selection_state)
                                            .with_selecting_start_and_rect(true, (mouse_x, mouse_y), Some((mouse_x, mouse_y, 0.0, 0.0))));
                                    } else {
                                        selection_state.set((*selection_state)
                                            .with_selecting_start_rect_and_blocks(true, (mouse_x, mouse_y), Some((mouse_x, mouse_y, 0.0, 0.0)), vec![]));
                                    }
                                }
                            }
                        } else if e.button() == 2 { // right button
                            selection_state.set((*selection_state).reset_selection());
                            // Right button panning
                            cursor_grabbing.set(true);
                            is_panning.set(true);
                            pan_start.set((e.client_x() as f32, e.client_y() as f32));
                        }
                    }
                }
            }
        })
    };

    // ----------------- Mouse move for pending line, block drag, canvas panning, marquee update -----------------
    let handle_canvas_mouse_move = {
        let pending_line = pending_line.clone();
        let drag_state = drag_state.clone();
        let blocks = blocks.clone();
        let connections = connections.clone();
        let canvas_ref = canvas_ref.clone();
        let is_panning = is_panning.clone();
        let selection_state = selection_state.clone();
        let pan_start = pan_start.clone();
        let canvas_offset = canvas_offset.clone();
        let last_frame = RefCell::new(0.0);
        let block_elements_ref = block_elements.clone();
        let conn_elements_ref = connection_elements.clone();

        Callback::from(move |e: MouseEvent| {
            let now = web_sys::js_sys::Date::now();
            if now - *last_frame.borrow() < 16.0 { return; }
            *last_frame.borrow_mut() = now;

            let client_x = e.client_x();
            let client_y = e.client_y();

            // Panning (right mouse)
            if *is_panning {
                let (start_x, start_y) = *pan_start;
                let dx = client_x as f32 - start_x;
                let dy = client_y as f32 - start_y;
                let (ox, oy) = *canvas_offset;
                canvas_offset.set((ox + dx, oy + dy));
                pan_start.set((client_x as f32, client_y as f32));
                return;
            }

            if let Some(canvas) = canvas_ref.cast::<HtmlElement>() {
                let rect = canvas.get_bounding_client_rect();
                let mouse_x = client_x as f32 - rect.left() as f32;
                let mouse_y = client_y as f32 - rect.top() as f32;

                // Pending line snap
                if let Some(((from_x, from_y), _)) = *pending_line {
                    let mut snapped = (mouse_x, mouse_y);
                    let (ox, oy) = *canvas_offset;
                    for block in blocks.borrow().iter() {
                        let port_x = block.position.0 + ox;
                        let port_y = block.position.1 + BLOCK_MIDDLE_Y + oy;
                        let dx = mouse_x - port_x;
                        let dy = mouse_y - port_y;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist < 10.0 { // Snap distance threshold
                            snapped = (port_x, port_y);
                        }
                    }
                    pending_line.set(Some(((from_x, from_y), snapped)));
                }

                if selection_state.is_selecting {
                    // compute normalized rect
                    let (start_x, start_y) = selection_state.selection_start;
                    let x = start_x.min(mouse_x);
                    let y = start_y.min(mouse_y);
                    let w = (mouse_x - start_x).abs();
                    let h = (mouse_y - start_y).abs();
                    let ctrl_key = e.ctrl_key();

                    // Update selected_blocks: block intersects rect?
                    let mut sel = Vec::new();
                    if ctrl_key {
                        selection_state.selected_blocks.iter().for_each(|b| sel.push(*b));
                    }
                    blocks.borrow().iter().filter(|b| b.intersects_rect((x, y), (x + w, y + h), *canvas_offset))
                        .for_each(|b| sel.push(b.id));

                    selection_state.set((*selection_state).with_rect_and_blocks(Some((x, y, w, h)), sel));
                }

                // Update dragging block (Single or Group)
                if let Some(block_id) = drag_state.block_id {
                    let mut block_elements = block_elements_ref.borrow_mut();
                    if block_elements.is_empty() {
                        if let Some(window) = web_sys::window() {
                            if let Some(document) = window.document() {
                                for block_id in &drag_state.dragging_group {
                                    if let Some(el) = document.get_element_by_id(&format!("block-{block_id}")) {
                                        let div = el.dyn_into::<HtmlElement>().unwrap();
                                        block_elements.insert(*block_id, div);
                                    }
                                }

                                if let std::collections::hash_map::Entry::Vacant(e) = block_elements.entry(block_id) {
                                    if let Some(el) = document.get_element_by_id(&format!("block-{block_id}")) {
                                        let div = el.dyn_into::<HtmlElement>().unwrap();
                                        e.insert(div);
                                    }
                                }

                                let mut conn_elements = conn_elements_ref.borrow_mut();
                                // find connections
                                for conn in connections.borrow().iter() {
                                    if block_id == conn.from || block_id == conn.to
                                        || drag_state.dragging_group.contains(&conn.from) || drag_state.dragging_group.contains(&conn.to) {
                                        if let Some(el) = document.get_element_by_id(&format!("conn-{}-{}", conn.from, conn.to)) {
                                            let path_el = el.dyn_into::<Element>().unwrap();
                                            conn_elements.insert((conn.from, conn.to), path_el);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let mut moved_blocks = HashSet::new();

                    // If the dragged block is member of a selection  -> move group
                    if (*drag_state.dragging_group).contains(&block_id) && !selection_state.group_initial_positions.is_empty() {
                        let (anchor_x, anchor_y) = selection_state.group_anchor_mouse;
                        let dx = mouse_x - anchor_x;
                        let dy = mouse_y - anchor_y;

                        let (ox, oy) = *canvas_offset;
                        let mut current_blocks = blocks.borrow_mut();
                        for (id, (ix, iy)) in &selection_state.group_initial_positions {
                            if let Some(b) = current_blocks.get_mut(*id as usize - 1) {
                                b.position = (ix + dx, iy + dy);
                                moved_blocks.insert(b.id);
                                if let Some(div) = block_elements.get(&b.id) {
                                    div.style().set_property("transform", &format!("translate({}px,{}px)", ix + dx + ox, iy + dy + oy)).unwrap();
                                }
                            }
                        }
                    } else {
                        // Single drag block
                        let (ox, oy) = *canvas_offset;
                        let (offset_x, offset_y) = drag_state.drag_offset;
                        let mut current_blocks = blocks.borrow_mut();
                        if let Some(block) = current_blocks.get_mut(block_id as usize - 1) {
                            block.position = (mouse_x - offset_x, mouse_y - offset_y);
                            moved_blocks.insert(block.id);
                            if let Some(div) = block_elements.get(&block_id) {
                                div.style().set_property("transform", &format!("translate({}px,{}px)", mouse_x - offset_x + ox, mouse_y - offset_y + oy)).unwrap();
                            }
                        }
                    }

                    if !moved_blocks.is_empty() {
                        let mut move_connections = HashMap::<String, (BlockId, BlockId)>::new();
                        for conn in connections.borrow().iter() {
                            if moved_blocks.contains(&conn.from) || moved_blocks.contains(&conn.to) {
                                move_connections.insert(format!("conn-{}-{}", conn.from, conn.to), (conn.from, conn.to));
                            }
                        }
                        let current_blocks = blocks.borrow();
                        let conn_elements = conn_elements_ref.borrow();
                        let (ox, oy) = *canvas_offset;
                        for (from, to) in move_connections.values() {
                            if let Some(path_el) = conn_elements.get(&(*from, *to)) {
                                if let (Some(from_block), Some(to_block)) =
                                    (&current_blocks.get(*from as usize - 1), &current_blocks.get(*to as usize - 1)) {
                                    let from_x = from_block.position.0 + BLOCK_WIDTH + ox;
                                    let from_y = from_block.position.1 + BLOCK_MIDDLE_Y + oy;
                                    let to_x = to_block.position.0 + ox;
                                    let to_y = to_block.position.1 + BLOCK_MIDDLE_Y + oy;
                                    let dx = to_x - from_x;
                                    let ctrl = dx * 0.5;
                                    let new_d = format!(
                                        "M {} {} C {} {}, {} {}, {} {}",
                                        from_x, from_y,
                                        from_x + ctrl, from_y,
                                        to_x - ctrl, to_y,
                                        to_x, to_y
                                    );
                                    path_el.set_attribute("d", &new_d).unwrap();
                                }
                            }
                        }
                    }
                }
            }
        })
    };

    let handle_canvas_mouse_up = {
        let drag_state = drag_state.clone();
        let is_panning = is_panning.clone();
        let selection_state = selection_state.clone();
        let cursor_grabbing = cursor_grabbing.clone();
        let block_elements = block_elements.clone();
        let connection_elements = connection_elements.clone();
        Callback::from(move |_e: MouseEvent| {
            block_elements.borrow_mut().clear();
            connection_elements.borrow_mut().clear();
            // Stop any block dragging and stop panning
            if drag_state.block_id.is_some() {
                drag_state.set((*drag_state).reset_dragging());
            }
            is_panning.set(false);
            cursor_grabbing.set(false);
            selection_state.set((*selection_state).with_selecting_rect_and_group_initial_positions(false, None, vec![]));
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

    // Deleting a Block means updating the following block ids,
    // because a BlockId is the index in the blocks list.
    let handle_delete_block = {
        let blocks = blocks.clone();
        let connections = connections.clone();
        let next_id = next_id.clone();
        let selection_state = selection_state.clone();
        Callback::from(move |block_id: BlockId| {
            let mut current_blocks = blocks.borrow_mut();
            current_blocks.retain(|b| b.id != block_id);

            let mut current_connections = connections.borrow_mut();
            current_connections.retain(|c| c.from != block_id && c.to != block_id);

            for block in current_blocks.iter_mut() {
                if block.id >= block_id {
                    block.id -= 1;
                }
            }

            // udpate connection ids
            for conn in current_connections.iter_mut() {
                if conn.from >= block_id {
                    conn.from -= 1;
                }
                if conn.to >= block_id {
                    conn.to -= 1;
                }
            }

            let max_id = current_blocks.iter().map(|b| b.id).max().unwrap_or(0);
            next_id.set(max_id + 1);

            selection_state.set((*selection_state).with_cleared_blocks(block_id));
        })
    };

    let handle_delete_connection = {
        let connections = connections.clone();
        Callback::from(move |(from, to): (BlockId, BlockId)| {
            connections.borrow_mut().retain(|c| !(c.from == from && c.to == to));
        })
    };

    let get_port_status = {
        |block: &Block| {
            if let Some(from_id) = *pending_connection {
                if let Some(from_block) = blocks.borrow().get(from_id as usize - 1) {
                    return if can_connect(from_block, block, &connections.borrow(), &blocks.borrow()) {
                        PortStatus::Valid
                    } else {
                        PortStatus::Invalid
                    };
                }
            }
            PortStatus::Inactive
        }
    };

    let form_changed = {
        let blocks = blocks.clone();
        Callback::<(BlockId, BlockInstance)>::from(move |(block_id, instance): (BlockId, BlockInstance)| {
            let mut current_blocks = blocks.borrow_mut();
            if let Some(block) = current_blocks.get_mut(block_id as usize - 1) {
                block.instance = instance;
            }
        })
    };

    let edit_mode = use_state(|| EditMode::Inactive);

    let handle_block_edit = {
        let edit_mode_set = edit_mode.clone();
        let blocks = blocks.clone();
        let selection_state = selection_state.clone();
        Callback::from(move |block_id: BlockId| {
            if let Some(block) = blocks.borrow().get(block_id as usize - 1) {
                edit_mode_set.set(EditMode::Active(block.clone()));
                selection_state.set((*selection_state).reset_selection());
            }
        })
    };

    let editor_context = SourceEditorContext {
        on_form_change: form_changed,
        edit_mode: edit_mode.clone(),
    };

    let edited_block_id = match *edit_mode {
        EditMode::Inactive => 0,
        EditMode::Active(ref b) => b.id,
    };
    let grabbed = *cursor_grabbing;
    let selection_mode = selection_state.is_selecting;
    // ----------------- Render -----------------
    html! {
        <ContextProvider<SourceEditorContext> context={editor_context}>
        <span>{"WORK IN PROGRESS - NOT FINALIZED !!!"}</span>
        <div class="tp__source-editor">
            <SourceEditorSidebar
                delete_mode={*delete_mode}
                on_drag_start={handle_drag_start.clone()}
                on_toggle_delete={handle_toggle_delete_mode.clone()}
                on_layout={handle_layout.clone()}
            />
            // Canvas
            <div class="tp__source-editor__canvas-wrapper">
            <div
                ref={canvas_ref.clone()}
                class={classes!("tp__source-editor__canvas", "graph-paper-advanced", if grabbed {"grabbed"} else {""}, if selection_mode {"selection_mode"} else {""})}
                ondrop={handle_drop.clone()}
                ondragend={handle_drag_end.clone()}
                ondragover={handle_drag_over.clone()}
                onmousemove={handle_canvas_mouse_move.clone()}
                onmousedown={handle_canvas_mouse_down.clone()}
                onmouseup={handle_canvas_mouse_up.clone()}
                oncontextmenu={handle_canvas_right_click.clone()}>

                // SVG for connections
                <svg class={classes!("tp__source-editor__connections", if grabbed {"grabbed"} else {""}, if selection_mode {"selection_mode"} else {""})}>
                    { for connections.borrow().iter().filter_map(|c| {
                        let current_blocks = blocks.borrow();
                        let from_block = current_blocks.get(c.from as usize -1)?;
                        let to_block = current_blocks.get(c.to as usize -1)?;
                        let (ox, oy) = *canvas_offset; // Apply virtual canvas offset
                        let from_x = from_block.position.0 + BLOCK_WIDTH + ox;
                        let from_y = from_block.position.1 + BLOCK_MIDDLE_Y + oy;
                        let to_x = to_block.position.0 + ox;
                        let to_y = to_block.position.1 + BLOCK_MIDDLE_Y + oy;
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
                                <path id={format!("conn-{}-{}", c.from, c.to)} d={d} stroke="var(--source-editor-line-color)" fill="transparent" stroke-width="2"/>
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

                    // Pending line straight
                    { if let Some(((x1, y1), (x2, y2))) = *pending_line {
                        html! {
                            <line
                                x1={x1.to_string()} y1={y1.to_string()}
                                x2={x2.to_string()} y2={y2.to_string()}
                                stroke="var(--source-editor-pending-line-color)"
                                stroke-width="2"
                                stroke-dasharray="4 2" />
                        }
                    } else { html!{} } }
                </svg>

                // Selection rectangle overlay
                {
                    if let Some((x, y, w, h)) = selection_state.selection_rect {
                        let style = format!("position:absolute; left:{}px; top:{}px; width:{}px; height:{}px; pointer-events:none;", x, y, w, h);
                        html! {
                            <div class="tp__source-editor__selection-rect" style={style}></div>
                        }
                    } else {
                        html! {}
                    }
                }

                // Render blocks with canvas offset
                { for blocks.borrow().iter().map(|b|{
                    let port_status = get_port_status(b);
                    let (ox, oy) = *canvas_offset; // Apply virtual offset to each block
                    let mut shifted_block = b.clone();
                    let block_id = shifted_block.id;
                    shifted_block.position = (b.position.0 + ox, b.position.1 + oy);
                    let is_block_selected = selection_state.selected_blocks.contains(&block_id);
                    html! {
                    <BlockView
                        key={block_id}
                        block={shifted_block}
                        edited={edited_block_id == block_id}
                        selected={is_block_selected}
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

