use log::info;
use shared::foundation::filter::Filter;
use yew::prelude::*;


// Indents with spaces for pretty printing
fn indent(level: usize, do_indent: bool) -> Html {
    if do_indent {
        let spaces: AttrValue = " ".repeat(level * 2).into();
        html! { <>{ spaces }</> }
    } else {
        html! {}
    }
}

fn newline() -> Html {
    html! { <br /> }
}

fn render_filter(filter: &Filter, level: usize, do_indent: bool, p_count: usize) -> Html {
    match filter {
        Filter::Group(inner) => {
            html! {
            <>
                { indent(level, do_indent) }
                <span class={format!("bracket bracket-{}", p_count % 6)}>{"("}</span>
                {newline()}
                { indent(level +1 , true) }
                { render_filter(inner, level + 1, false, p_count+1) }
                {newline()}
                { indent(level , true) }
                <span class={format!("bracket bracket-{}", p_count % 6)}>{ ")" }</span>
            </>
         }
        }
        Filter::FieldComparison(field, regex) => html! {
            <>
               { indent(level, do_indent) }
                <span class="comparison">
                    <span class="field">{format!("{:?}", field)}</span>
                    {" ~ "}
                    <span class="regex">{format!("\"{}\"", regex.restr)}</span>
                </span>
            </>
        },
        Filter::TypeComparison(field, t) => html! {
            <>
               { indent(level, do_indent) }
                <span class="comparison">
                    <span class="field">{format!("{:?}", field)}</span>{" = "}
                    <span class="enum">{format!("{:?}", t)}</span>
                </span>
            </>
        },
        Filter::UnaryExpression(op, inner) => {
            html! {
                <>
                    { indent(level, do_indent) }
                    <span class="unary_op">{format!(" {:?} ", op)}</span>
                    {newline()}
                    { indent(level, true) }
                    { render_filter(inner, level + 1, do_indent, p_count) }
                </>
            }
        },
        Filter::BinaryExpression(left, op, right) => html! {
            <span class="binary_op-wrapper">
                 { render_filter(left, level, do_indent, p_count) }
                 { newline() }
                 { indent(level, true) }
                 <span class="binary_op">{format!(" {:?} ", op)}</span>
                 { newline() }
                 { render_filter(right, level, true, p_count) }
            </span>
        },
    }
}

#[derive(Properties, PartialEq, Clone)]
pub struct FilterViewProps {
    pub filter: Option<Filter>,
}

#[function_component]
pub fn FilterView(props: &FilterViewProps) -> Html {
    html! {
        <div class="tp__filter">
            {
                match props.filter.as_ref() {
                    Some(filter) => html! {
                        <pre class="tp__filter__code">
                            { render_filter(filter, 0, false, 1) }
                        </pre>
                    },
                    None => html! { },
                }
            }
        </div>
    }
}