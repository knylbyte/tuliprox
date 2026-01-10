use crate::model::{ConfigSortRule, ConfigTarget};
use std::cmp::Ordering;
use shared::foundation::filter::{ValueProvider};
use shared::model::{PlaylistGroup, SortOrder, SortTarget};

fn direction(order: SortOrder, ordering: Ordering) -> Ordering {
    match order {
        SortOrder::Asc => if ordering == Ordering::Less { Ordering::Less } else { Ordering::Greater },
        SortOrder::Desc => if ordering == Ordering::Less { Ordering::Greater } else { Ordering::Less },
        SortOrder::None => Ordering::Equal,
    }
}

fn playlist_comparator(
    sequence: Option<&Vec<regex::Regex>>,
    order: SortOrder,
    value_a: &str,
    value_b: &str,
) -> Ordering {
    if matches!(order, SortOrder::None) {
        return Ordering::Equal;
    }
    if let Some(regex_list) = sequence {
        let mut match_a = None;
        let mut match_b = None;

        for (i, regex) in regex_list.iter().enumerate() {
            if match_a.is_none() {
                if let Some(caps) = regex.captures(value_a) {
                    match_a = Some((i, caps));
                }
            }
            if match_b.is_none() {
                if let Some(caps) = regex.captures(value_b) {
                    match_b = Some((i, caps));
                }
            }

            // If both matches found → break
            if match_a.is_some() && match_b.is_some() {
                break;
            }
        }

        match (match_a, match_b) {
            (Some((idx_a, caps_a)), Some((idx_b, caps_b))) => {
                // Different regex indices → sort by their sequence order.
                if idx_a != idx_b {
                    return match order {
                        SortOrder::Asc => idx_a.cmp(&idx_b),
                        SortOrder::Desc => idx_b.cmp(&idx_a),
                        SortOrder::None => Ordering::Equal,
                    };
                }

                // Same regex → sort by captures (c1, c2, …)
                let mut named: Vec<_> = regex_list[idx_a]
                    .capture_names()
                    .flatten()
                    .filter(|name| name.starts_with('c'))
                    .collect();

                named.sort_by_key(|name| name[1..].parse::<u32>().unwrap_or(0));

                for name in named {
                    let va = caps_a.name(name).map(|m| m.as_str());
                    let vb = caps_b.name(name).map(|m| m.as_str());
                    if let (Some(va), Some(vb)) = (va, vb) {
                        let o = va.cmp(vb);
                        if !matches!(o, Ordering::Equal) {
                            return match order {
                                SortOrder::Asc => o,
                                SortOrder::Desc => o.reverse(),
                                SortOrder::None => Ordering::Equal,
                            };
                        }
                    }
                }

                let o = value_a.cmp(value_b);
                match order {
                    SortOrder::Asc => o,
                    SortOrder::Desc => o.reverse(),
                    SortOrder::None => Ordering::Equal,
                }
            }
            (Some(_), None) => direction(order, Ordering::Less),
            (None, Some(_)) => direction(order, Ordering::Greater),
            (None, None) => {
                // NP match → fallback
                let o = value_a.cmp(value_b);
                match order {
                    SortOrder::Asc => o,
                    SortOrder::Desc => o.reverse(),
                    SortOrder::None => Ordering::Equal,
                }
            }
        }
    } else {
        // No Regex-Sequence defined → fallback
        let o = value_a.cmp(value_b);
        match order {
            SortOrder::Asc => o,
            SortOrder::Desc => o.reverse(),
            SortOrder::None => Ordering::Equal,
        }
    }
}

pub(in crate::processing::processor) fn sort_playlist(target: &ConfigTarget, playlist: &mut [PlaylistGroup]) -> bool {
    let Some(sort) = &target.sort else {
        return false;
    };

    let rules = &sort.rules;
    let match_as_ascii = sort.match_as_ascii;
    sort_groups(playlist, rules, match_as_ascii);
    sort_channels_in_groups(playlist, rules, match_as_ascii);

    true
}

fn sort_groups(
    groups: &mut [PlaylistGroup],
    rules: &[ConfigSortRule],
    match_as_ascii: bool,
) {
    let group_rules: Vec<_> = rules
        .iter()
        .filter(|r| matches!(r.target, SortTarget::Group))
        .collect();

    if group_rules.is_empty() {
        return;
    }

    groups.sort_by(|a_grp, b_grp| {
        for rule in &group_rules {
            if rule.order == SortOrder::None {
                continue;
            }

            let (Some(a_chan), Some(b_chan)) = (a_grp.channels.first(), b_grp.channels.first()) else {
                continue;
            };

            let provider_a = ValueProvider { pli: a_chan, match_as_ascii };
            let provider_b = ValueProvider { pli: b_chan, match_as_ascii };

            match (rule.filter.filter(&provider_a), rule.filter.filter(&provider_b)) {
                (false, false) => return Ordering::Equal,
                (true, false) =>  return direction(rule.order, Ordering::Less),
                (false, true) => return direction(rule.order, Ordering::Greater),
                (true, true) => { /* fallthrough */}
            }

            let va = provider_a.get(rule.field.as_str());
            let vb = provider_b.get(rule.field.as_str());
            match (va, vb) {
                (None, None) => return Ordering::Equal,
                (Some(_), None) => return direction(rule.order, Ordering::Less),
                (None, Some(_)) => return direction(rule.order, Ordering::Greater),
                (Some(va), Some(vb)) => {
                    let ord = playlist_comparator(
                        rule.sequence.as_ref(),
                        rule.order,
                        &va,
                        &vb,
                    );

                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
            }
        }

        Ordering::Equal
    });
}

fn sort_channels_in_groups(
    groups: &mut [PlaylistGroup],
    rules: &[ConfigSortRule],
    match_as_ascii: bool,
) {
    let channel_rules: Vec<_> = rules
        .iter()
        .filter(|r| matches!(r.target, SortTarget::Channel))
        .collect();

    if channel_rules.is_empty() {
        return;
    }

    for group in groups {
        group.channels.sort_by(|a, b| {
            for rule in &channel_rules {
                if rule.order == SortOrder::None {
                    continue;
                }

                let provider_a = ValueProvider { pli: a, match_as_ascii };
                let provider_b = ValueProvider { pli: b, match_as_ascii };

                match (rule.filter.filter(&provider_a), rule.filter.filter(&provider_b)) {
                    (false, false) => continue,
                    (true, false) => return direction(rule.order, Ordering::Less),
                    (false, true) => return direction(rule.order, Ordering::Greater),
                    (true, true) => {}
                }

                let va = provider_a.get(rule.field.as_str());
                let vb = provider_b.get(rule.field.as_str());
                match (va, vb) {
                    (None, None) => return Ordering::Equal,
                    (Some(_), None) => return direction(rule.order, Ordering::Less),
                    (None, Some(_)) => return direction(rule.order, Ordering::Greater),
                    (Some(va), Some(vb)) => {
                        let ord = playlist_comparator(
                            rule.sequence.as_ref(),
                            rule.order,
                            &va,
                            &vb,
                        );

                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                }
            }

            a.header.source_ordinal.cmp(&b.header.source_ordinal)
        });
    }
}



#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use regex::Regex;
    use shared::foundation::filter::Filter;
    use shared::model::{ItemField, PlaylistItem, PlaylistItemHeader, SortOrder, SortTarget};
    use crate::model::ConfigSortRule;
    use crate::processing::processor::sort::playlist_comparator;

    #[test]
    fn test_sort() {
        let mut channels: Vec<PlaylistItem> = vec![
            ("D", "HD"), ("A", "FHD"), ("Z", "HD"), ("K", "HD"), ("B", "HD"), ("A", "HD"),
            ("K", "UHD"), ("C", "HD"), ("L", "FHD"), ("R", "UHD"), ("T", "SD"), ("A", "FHD"),
        ]
            .into_iter()
            .enumerate()
            .map(|(i, (name, quality))| PlaylistItem {
                header: PlaylistItemHeader {
                    title: format!("Chanel {name} [{quality}]"),
                    source_ordinal: i as u32,
                    ..Default::default()
                },
            })
            .collect();

        let channel_sort = ConfigSortRule {
            target: SortTarget::Channel,
            field: ItemField::Caption,
            order: SortOrder::Asc,
            sequence: Some(vec![
                Regex::new(r"(?P<c1>.*?)\bUHD\b").unwrap(),
                Regex::new(r"(?P<c1>.*?)\bFHD\b").unwrap(),
                Regex::new(r"(?P<c1>.*?)\bHD\b").unwrap(),
            ]),
            filter: Filter::default(),
        };

        channels.sort_by(|a, b| {
            let va = &a.header.title;
            let vb = &b.header.title;

            let ord = playlist_comparator(
                channel_sort.sequence.as_ref(),
                channel_sort.order,
                va,
                vb,
            );

            if ord != Ordering::Equal {
                ord
            } else {
                a.header.source_ordinal.cmp(&b.header.source_ordinal)
            }
        });

        let expected = vec![
            "Chanel K [UHD]",
            "Chanel R [UHD]",
            "Chanel A [FHD]",
            "Chanel A [FHD]",
            "Chanel L [FHD]",
            "Chanel A [HD]",
            "Chanel B [HD]",
            "Chanel C [HD]",
            "Chanel D [HD]",
            "Chanel K [HD]",
            "Chanel Z [HD]",
            "Chanel T [SD]",
        ];

        let sorted = channels
            .into_iter()
            .map(|pli| pli.header.title)
            .collect::<Vec<_>>();

        assert_eq!(expected, sorted);
    }

    #[test]
    fn test_sort2() {
        let mut channels: Vec<PlaylistItem> = vec![
            "US| EAST [FHD] abc",
            "US| EAST [FHD] def",
            "US| EAST [FHD] ghi",
            "US| EAST [HD] jkl",
            "US| EAST [HD] mno",
            "US| EAST [HD] pqrs",
            "US| EAST [HD] tuv",
            "US| EAST [HD] wxy",
            "US| EAST [HD] z",
            "US| EAST [SD] a",
            "US| EAST [FHD] bc",
            "US| EAST [FHD] de",
            "US| EAST [HD] f",
            "US| EAST [HD] h",
            "US| EAST [SD] ijk",
            "US| EAST [SD] l",
            "US| EAST [UHD] m",
            "US| WEST [FHD] no",
            "US| WEST [HD] qrst",
            "US| WEST [HD] uvw",
            "US| (West) xv",
            "US| East d",
            "US| West e",
            "US| West f",
        ]
            .into_iter()
            .enumerate()
            .map(|(i, name)| PlaylistItem {
                header: PlaylistItemHeader {
                    title: name.to_string(),
                    source_ordinal: i as u32,
                    ..Default::default()
                },
            })
            .collect();

        let channel_sort = ConfigSortRule {
            target: SortTarget::Channel,
            field: ItemField::Caption,
            order: SortOrder::Asc,
            sequence: Some(vec![
                Regex::new(r"^US\| EAST.*?\[\bUHD\b\](?P<c1>.*)").unwrap(),
                Regex::new(r"^US\| EAST.*?\[\bFHD\b\](?P<c1>.*)").unwrap(),
                Regex::new(r"^US\| EAST.*?\[\bHD\b\](?P<c1>.*)").unwrap(),
                Regex::new(r"^US\| EAST.*?\[\bSD\b\](?P<c1>.*)").unwrap(),
                Regex::new(r"^US\| WEST.*?\[\bUHD\b\](?P<c1>.*)").unwrap(),
                Regex::new(r"^US\| WEST.*?\[\bFHD\b\](?P<c1>.*)").unwrap(),
                Regex::new(r"^US\| WEST.*?\[\bHD\b\](?P<c1>.*)").unwrap(),
                Regex::new(r"^US\| WEST.*?\[\bSD\b\](?P<c1>.*)").unwrap(),
            ]),
            filter: Filter::default(),
        };

        channels.sort_by(|a, b| {
            let ord = playlist_comparator(
                channel_sort.sequence.as_ref(),
                channel_sort.order,
                &a.header.title,
                &b.header.title,
            );

            if ord != Ordering::Equal {
                ord
            } else {
                a.header.source_ordinal.cmp(&b.header.source_ordinal)
            }
        });

        let expected = vec![
            "US| EAST [UHD] m",
            "US| EAST [FHD] abc",
            "US| EAST [FHD] bc",
            "US| EAST [FHD] de",
            "US| EAST [FHD] def",
            "US| EAST [FHD] ghi",
            "US| EAST [HD] f",
            "US| EAST [HD] h",
            "US| EAST [HD] jkl",
            "US| EAST [HD] mno",
            "US| EAST [HD] pqrs",
            "US| EAST [HD] tuv",
            "US| EAST [HD] wxy",
            "US| EAST [HD] z",
            "US| EAST [SD] a",
            "US| EAST [SD] ijk",
            "US| EAST [SD] l",
            "US| WEST [FHD] no",
            "US| WEST [HD] qrst",
            "US| WEST [HD] uvw",
            "US| (West) xv",
            "US| East d",
            "US| West e",
            "US| West f",
        ];

        let sorted = channels
            .into_iter()
            .map(|pli| pli.header.title)
            .collect::<Vec<_>>();

        assert_eq!(expected, sorted);
    }

}
