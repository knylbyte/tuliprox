use log::error;
use yew::prelude::*;
use serde::{Serialize, Deserialize};
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement};
// ----------------- Data Models -----------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum BlockType {
    Task,
    Decision,
    ApiCall,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub id: usize,
    pub block_type: BlockType,
    pub position: (f32, f32),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub from: usize,
    pub to: usize,
}

#[function_component]
pub fn SourceEditor() -> Html {
    let blocks = use_state(|| Vec::<Block>::new());
    let connections = use_state(|| Vec::<Connection>::new());
    let next_id = use_state(|| 1usize);

    // ----------------- Drag Start -----------------
    let on_drag_start = {
        Callback::from(|e: DragEvent| {
            if let Some(target) = e.target_dyn_into::<HtmlElement>() {
                let block_type = target.get_attribute("data-block-type").unwrap_or_default();
                e.data_transfer().unwrap().set_data("text/plain", &block_type).unwrap();
            }
        })
    };

    // ----------------- Drop on Canvas -----------------
    let on_drop = {
        let blocks = blocks.clone();
        let next_id = next_id.clone();
        Callback::from(move |e: DragEvent| {
            e.prevent_default();
            if let Some(current_target) = e.current_target() {
                if let Ok(canvas) = current_target.dyn_into::<HtmlElement>() {
                    if let Some(data) = e.data_transfer().unwrap().get_data("text/plain").ok() {
                        error!("data transfer {data}");
                        let x = e.client_x() as f32 - canvas.offset_left() as f32;
                        let y = e.client_y() as f32 - canvas.offset_top() as f32;
                        let block_type = match data.as_str() {
                            "Task" => BlockType::Task,
                            "Decision" => BlockType::Decision,
                            "ApiCall" => BlockType::ApiCall,
                            _ => BlockType::Task,
                        };

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
            }
        })
    };

    // ----------------- Drag Over Canvas -----------------
    let on_drag_over = Callback::from(|e: DragEvent| e.prevent_default());

    html! {
        <div class="tp__source-editor">
            // Sidebar with block palette
            <div style="tp__source-editor__sidebar">
                <div
                    class="tp__source-editor__brick"
                    draggable={"true"}
                    data-block-type="Task"
                    ondragstart={on_drag_start.clone()}>
                    { "Task" }
                </div>
                <div
                    class="tp__source-editor__brick"
                    draggable={"true"}
                    data-block-type="Decision"
                    ondragstart={on_drag_start.clone()}>
                    { "Decision" }
                </div>
                <div
                    class="tp__source-editor__brick"
                    draggable={"true"}
                    data-block-type="ApiCall"
                    ondragstart={on_drag_start}>
                    { "ApiCall" }
                </div>
            </div>

            // Canvas area
            <div
                class="tp__source-editor__canvas"
                ondrop={on_drop}
                ondragover={on_drag_over}>
                // Draw connections (lines)
                <svg style="position:absolute; width:100%; height:100%;">
                    { for (*connections).iter().map(|c| {
                        let from_block = (*blocks).iter().find(|b| b.id == c.from).unwrap();
                        let to_block = (*blocks).iter().find(|b| b.id == c.to).unwrap();
                        html! {
                            <line
                                x1={from_block.position.0.to_string()}
                                y1={from_block.position.1.to_string()}
                                x2={to_block.position.0.to_string()}
                                y2={to_block.position.1.to_string()}
                                stroke="white"
                                stroke-width="2"
                            />
                        }
                    }) }
                </svg>

                // Render blocks
                { for (*blocks).iter().map(|b| {
                    let style = format!( "position:absolute; left:{}px; top:{}px;", b.position.0, b.position.1);
                    html! {
                        <div class="tp__source-editor__block" {style}>{ format!("{:?}", b.block_type) }</div>
                    }
                }) }
            </div>
        </div>
    }
}
