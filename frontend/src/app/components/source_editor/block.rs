use web_sys::MouseEvent;
use yew::{classes, function_component, html, Callback, Html, Properties};
use yew_i18n::use_translation;
use crate::html_if;
use crate::app::components::{Block, BlockInstance, BlockType, PortStatus};
#[derive(Properties, PartialEq)]
pub struct BlockProps {
    pub(crate) block: Block,
    pub(crate) delete_mode: bool,
    pub(crate) delete_block: Callback<usize>,
    pub(crate) port_status: PortStatus,
    pub(crate) on_edit: Callback<usize>,
    pub(crate) on_mouse_down: Callback<(usize, MouseEvent)>,
    pub(crate) on_connection_drop: Callback<usize>, // to_id
    pub(crate) on_connection_start:  Callback<usize>, // from_id
}

#[function_component]
pub fn BlockView(props: &BlockProps) -> Html {

    let translate = use_translation();

    let delete_mode = props.delete_mode;
    let delete_block = props.delete_block.clone();
    let block = &props.block;
    let port_status = props.port_status;

    let block_id = block.id;
    let block_type = block.block_type;
    let style = format!("position:absolute; left:{}px; top:{}px;", block.position.0, block.position.1);
    let from_id = block_id;
    let to_id = block_id;

    let is_target = matches!(block_type, BlockType::Target);
    let is_input = !is_target && matches!(block_type, BlockType::InputM3u | BlockType::InputXtream);
    let is_output =  !is_input && !is_target;

    let port_style = match port_status {
        PortStatus::Valid =>  "tp__source-editor__block-port--valid",
        PortStatus::Invalid =>  "tp__source-editor__block-port--invalid",
        _ => "",
    };

    let handle_mouse_down = {
        let on_block_mouse_down = props.on_mouse_down.clone();
        Callback::from(move |e| on_block_mouse_down.emit((block_id, e)))
    };

    let handle_edit = {
        let block_id = block_id;
        let on_edit = props.on_edit.clone();
        Callback::from(move |_| {
           on_edit.emit(block_id)
        })
    };

    let (title, show_type) = {
        let (dto_title, show_type) = match &block.instance {
            BlockInstance::Input(dto) => (dto.name.clone(), true),
            BlockInstance::Target(dto) => (dto.name.clone(), true),
            BlockInstance::Output(_output) => {
                (translate.t(&format!("SOURCE_EDITOR.BRICK_{}", block_type)), false)
            }
        };
        if dto_title.is_empty() {
            (translate.t(&format!("SOURCE_EDITOR.BRICK_{}", block_type)), false)
        } else {
            (dto_title, show_type)
        }
    };

    html! {
        <div class={format!("tp__source-editor__block no-select tp__source-editor__block-{}", block_type)} style={style}>
            <div class={"tp__source-editor__block-header"}>
                // Block handle (drag)
                <div class="tp__source-editor__block-handle" onmousedown={handle_mouse_down}/>
                // Delete button for block
                {
                    html_if!(delete_mode, {
                        <div class={"tp__source-editor__block-header-actions"}>
                        <div class="tp__source-editor__block-delete" onclick={
                            Callback::from(move |_| delete_block.emit(block_id))
                        }></div>
                        </div>
                    })
                }
            </div>
            <div class={"tp__source-editor__block-content"} ondblclick={handle_edit}>
                <div class={"tp__source-editor__block-content-body"}>
                    <div class="tp__source-editor__block-label">
                        { title }
                    </div>
                    {
                        html_if!(show_type, {
                          <span class="tp__source-editor__block-sub-label">{translate.t(&format!("SOURCE_EDITOR.BRICK_{}", block_type))}</span>
                        })
                    }
                </div>

               {html_if!(is_target || is_output, {
                // Left port
                <div
                    class={classes!("tp__source-editor__block-port", "tp__source-editor__block-port--left", port_style)}
                    onmouseup={{
                        let on_connection_drop = props.on_connection_drop.clone();
                        Callback::from(move |_| on_connection_drop.emit(to_id))
                    }} />
                })}

               {html_if!(is_target || is_input, {
                // Right port
                <div
                    class="tp__source-editor__block-port tp__source-editor__block-port--right"
                    onmousedown={{
                        let on_connection_start = props.on_connection_start.clone();
                        Callback::from(move |_| on_connection_start.emit(from_id))
                    }} />
                })}
            </div>
        </div>
    }
}