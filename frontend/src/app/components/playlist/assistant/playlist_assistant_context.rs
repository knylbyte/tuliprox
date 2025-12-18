use yew::UseStateHandle;

#[allow(dead_code)]
#[derive(Clone, PartialEq)]
pub struct PlaylistAssistantContext {
    pub custom_class: UseStateHandle<String>,
}
