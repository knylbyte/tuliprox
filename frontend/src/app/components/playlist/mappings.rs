use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{MapperDto, MappingCounterDefinition, MappingDto};
use crate::app::components::{ConfigContext, FilterView, MapperScriptView, NoContent};
use crate::app::components::toggle_switch::ToggleSwitch;

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

    let render_mapper = |mapper: &MapperDto| {
        html! {
            <div class="tp__playlist-mappings__mapping-mapper-content">
                <FilterView filter={mapper.t_filter.clone()} />
                <MapperScriptView script={mapper.t_script.clone()} pretty={true}/>
            </div>
        }
    };

    let render_counter = |mapper: &MappingCounterDefinition| {
        html! {
            <div class="tp__playlist-mappings__mapper-counter">
                <label>{translate.t("LABEL.MAPPER")}</label>

            </div>
        }
    };

    let render_mapping = |mapping: &MappingDto| {
        html! {
            <div class="tp__playlist-mappings__mapping">
                <div class="tp__playlist-mappings__mapping-section">
                    <label>{translate.t("LABEL.ID")}</label>
                    {mapping.id.clone()}
                </div>
                <div class="tp__playlist-mappings__mapping-section">
                    <label>{translate.t("LABEL.MATCH_AS_ASCII")}</label>
                    <ToggleSwitch value={mapping.match_as_ascii} readonly={true} />
                </div>
                <div class="tp__playlist-mappings__mapping-mapper">
                    {
                        for mapping.mapper.iter().flatten().map(|mapper| {
                            html! { render_mapper(mapper) }
                        })
                    }
                </div>
                 <div class="tp__playlist-mappings__mapping-counter">
                    {
                        for mapping.counter.iter().flatten().map(|counter| {
                           html! { render_counter(counter) }
                        })
                    }
                </div>
            </div>
        }
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