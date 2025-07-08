use crate::app::components::{convert_bool_to_chip_style, Card, Chip};
use shared::model::{ConfigTargetDto, ItemField, SortOrder};
use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct TargetSortProps {
    pub target: Rc<ConfigTargetDto>,
}

#[function_component]
pub fn TargetSort(props: &TargetSortProps) -> Html {
    let translator = use_translation();

    let sort = match props.target.sort.as_ref() {
        Some(s) => s,
        None => return html! {},
    };

    let groups_html = match sort.groups.as_ref() {
        Some(groups) => {
            match groups.sequence.as_ref() {
                Some(seq) => html! {
                    <Card>
                    <h2>{translator.t("LABEL.GROUPS")}</h2>
                    <div class="tp__target-sort__section tp__target-sort__row">
                        <span class="tp__target-sort__label">{translator.t("LABEL.ORDER")}</span>
                        <span>{ groups.order.to_string() }</span>
                    </div>
                    <div class="tp__target-sort__section  tp__target-sort__row">
                        <span class="tp__target-sort__label">{translator.t("LABEL.SEQUENCE")}</span>
                        <span class="tp__target-sort__sequence">
                            <ul>
                                { for seq.iter().map(|p| html! { <li>{p}</li> }) }
                            </ul>
                        </span>
                    </div>
                    </Card>
                },
                None => html! {},
            }
        },
        None => html! {},
    };

    html! {
        <div class="tp__target-sort">
            <h2>{translator.t("LABEL.SORT_SETTINGS")}</h2>
            <div class="tp__target-sort__section  tp__target-sort__row">
                <Chip class={ convert_bool_to_chip_style(sort.match_as_ascii) }
                      label={translator.t("LABEL.MATCH_AS_ASCII")} />
            </div>
            { groups_html }
        </div>
    }
}

//   </div>
//   <div class="tp__target-output__output__section tp__target-output__output__row">
//       <span class="tp__target-output__output__label">{translator.t("LABEL.USERNAME")}</span>
//       <span>{ props.output.username.clone() }</span>
//   </div>
//   <div class="tp__target-output__output__section tp__target-output__output__row">
//       <span class="tp__target-output__output__label">{translator.t("LABEL.USE_OUTPUT")}</span>
//       <span>{ props.output.use_output.map_or_else(String::new, |o| o.to_string()) }</span>
//   </div>
// </div>

// pub match_as_ascii: bool,
// pub groups: Option<ConfigSortGroupDto>,
// pub channels: Option<Vec<ConfigSortChannelDto>>,

//
// #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
// #[serde(deny_unknown_fields)]
// pub struct ConfigSortGroupDto {
//     pub order: SortOrder,
//     #[serde(default, skip_serializing_if = "Option::is_none")]
//     pub sequence: Option<Vec<String>>,
//     #[serde(skip)]
//     pub t_sequence: Option<Vec<Regex>>,
// }

// pub struct ConfigSortChannelDto {
//     // channel field
//     pub field: ItemField,
//     // match against group title
//     pub group_pattern: String,
//     pub order: SortOrder,
//     #[serde(default, skip_serializing_if = "Option::is_none")]
//     pub sequence: Option<Vec<String>>,
//     #[serde(skip)]
//     pub t_sequence: Option<Vec<Regex>>,
// }
