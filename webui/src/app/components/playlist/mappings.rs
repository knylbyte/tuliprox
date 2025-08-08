use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::MappingDto;
use crate::app::components::{ConfigContext, NoContent};

#[derive(Properties, PartialEq, Clone)]
pub struct PlaylistMappingsProps {
    pub mappings: Option<Vec<String>>,
}

#[function_component]
pub fn PlaylistMappings(props: &PlaylistMappingsProps) -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");
    let mappings = use_memo(config_ctx.clone(), |context| {
        match &props.mappings {
            Some(mapping_ids) => {
                context.config.as_ref()
                    .and_then(|c| c.mappings.as_ref())
                    .map(|mappings_dto| {
                        mappings_dto.mappings.mapping.iter()
                            .filter(|m| mapping_ids.contains(&m.id))
                            .cloned()
                            .collect::<Vec<MappingDto>>()
                    })
            }
            None => None,
        }
    });

    let render_mapping = |mapping: &MappingDto| {
        html! {
            <div class="tp__playlist-mappings__mapping">
                <div class="tp__playlist-mappings__mapping-section">
                    <label>{translate.t("LABEL.ID")}</label>
                    {mapping.id.clone()}
                </div>
            </div>
        }
        //
        // pub id: String,
        // #[serde(default)]
        // pub match_as_ascii: bool,
        // pub mapper: Option<Vec<MapperDto>>,
        // pub counter: Option<Vec<MappingCounterDefinition>>,
        // #[serde(skip_serializing, skip_deserializing)]
        // pub t_counter: Option<Vec<MappingCounter>>,
        // #[serde(skip_serializing, skip_deserializing)]
        // pub templates: Option<Vec<PatternTemplate>>
    };

    html! {
      <div class="tp__playlist-mappings">
        {
            match (*mappings).as_ref() {
                Some(vec) => {
                    html! { for vec.iter().map(render_mapping) }
                },
                None => html! { <NoContent/>},
            }
        }
      </div>
    }
}