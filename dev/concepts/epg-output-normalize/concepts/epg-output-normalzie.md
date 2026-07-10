# Konzept: optionale EPG-Output-Normalisierung

- **Status:** Proposed
- **Zielbranch:** `develop`
- **Geprüfte Codebasis:** `knylbyte/tuliprox@4d0533ff00c15f15a71f49c30ef65b32f4c6b405`
- **Verwandte Änderung:** `euzu/tuliprox#783` – case-insensitives ASCII-Matching von EPG-IDs bei Erhalt der ursprünglichen Schreibweise
- **Konfigurationsumfang:** pro Target

## 1. Entscheidung nach Effizienz- und Sicherheitsprüfung

Die Output-Normalisierung wird **nicht** in den einzelnen M3U- und Xtream-Renderern ausgeführt. Stattdessen wird die `epg_channel_id` einmalig in der bereits fertig verarbeiteten Target-Playlist kanonisiert, unmittelbar bevor die Target-Ausgaben persistiert werden.

Dadurch gilt:

1. M3U und Xtream übernehmen automatisch dieselbe kanonische `epg_channel_id`.
2. Es entstehen keine zusätzlichen Lowercase-Operationen bei jedem Playlist-Download oder API-Aufruf.
3. M3U-, Xtream- und XMLTV-Ausgaben können nicht durch getrennte Renderer-Implementierungen auseinanderlaufen.
4. Smart Match und die Eingabeparser bleiben unverändert.
5. Die XMLTV-`display-name`-Normalisierung bleibt bewusst eine reine Serialisierungsoption und verändert weder Playlist-Namen noch den gespeicherten EPG-Titel.

Gegenüber dem ersten Entwurf ergeben sich damit folgende Korrekturen:

- keine Änderung an `M3uPlaylistItem::to_m3u()`;
- keine Erweiterung von `XtreamMappingOptions`;
- keine Änderung an `shared/src/model/playlist_document.rs` für `epg_channel_id`;
- kein Verschieben des ID-Helfers nach `shared`;
- einmalige Target-Normalisierung statt Normalisierung auf jedem Ausgabeweg;
- explizite Trennung zwischen ASCII-ID-Normalisierung und Unicode-Normalisierung des XMLTV-Anzeigenamens;
- Normalisierung eingehender EPG-Abfrage-IDs an den API-Grenzen;
- case-insensitives Rename-Map-Lookup passend zum Verhalten aus PR #783.

## 2. Problemstellung

Tuliprox führt seit PR #783 den internen Abgleich von EPG-Kanal-IDs ASCII-case-insensitiv durch. Eine Playlist-ID wie

```text
rtlzwei.de
```

kann dadurch intern zu einer XMLTV-ID wie

```text
RTLzwei.de
```

passen. Die originale Schreibweise wird jedoch absichtlich beibehalten. Daraus können unterschiedliche IDs in den ausgegebenen Artefakten entstehen:

```text
M3U/Xtream epg_channel_id: rtlzwei.de
XMLTV channel id:          RTLzwei.de
```

Einige Clients vergleichen diese Werte case-sensitiv. Zusätzlich arbeiten die Target-EPG-Datenbanken und die Runtime-EPG-Abfragen mit exakten B+Tree-Schlüsseln. Deshalb muss eine optionale Lowercase-Ausgabe nicht nur die sichtbare XMLTV-Datei verändern, sondern die gesamte Target-Ausgabe konsistent kanonisieren.

## 3. Ziele

Bei aktivierter Option sollen folgende Werte ASCII-lowercase ausgegeben bzw. gespeichert werden:

- `PlaylistItemHeader.epg_channel_id` der finalen Target-Playlist;
- M3U `tvg-id`;
- Xtream `epg_channel_id`;
- Target-EPG-DB-Schlüssel;
- `EpgChannel.id` in der Target-EPG-DB;
- XMLTV `<channel id="…">`;
- XMLTV `<programme channel="…">`;
- IDs für Short-EPG- und Stream-EPG-Abfragen sowie deren Antworten.

Optional soll ausschließlich der XMLTV-Anzeigename Unicode-lowercase ausgegeben werden:

- XMLTV `<display-name>…</display-name>`.

Das bestehende Verhalten bleibt vollständig erhalten, wenn die Optionen fehlen oder `false` sind.

## 4. Nicht-Ziele

Die Änderung darf folgende Daten nicht automatisch lowercasen:

- Playlist-`name`, `title` oder `caption`;
- M3U `tvg-name`;
- Xtream `name`;
- Gruppenbezeichnungen;
- XMLTV-Programmtitel;
- XMLTV-Programmbeschreibungen;
- Logos, URLs oder Catch-up-Attribute;
- IDs oder Namen in den Input-Caches;
- Smart-Match-Normalisierungsergebnisse.

Es erfolgt kein Regex-basiertes Umschreiben einer XML-Datei und keine Änderung der XMLTV-Parsersemantik.

## 5. Vorgeschlagene Konfiguration

Die Option gehört unter die Target-Optionen:

```yaml
sources:
  - inputs:
      - my_provider
    targets:
      - name: my_target
        options:
          epg_output:
            lowercase_ids: true
            lowercase_xmltv_display_names: true
        output:
          - type: m3u
          - type: xtream
```

### 5.1 Semantik

| Option | Default | Wirkung |
|---|---:|---|
| `epg_output.lowercase_ids` | `false` | Kanonisiert sämtliche ausgegebenen EPG-IDs eines Targets mit ASCII-Lowercase. |
| `epg_output.lowercase_xmltv_display_names` | `false` | Wandelt ausschließlich XMLTV-`display-name` mit Unicode-Lowercase um. |

### 5.2 Warum Target-Level statt Output-Level?

Ein Target kann gleichzeitig M3U und Xtream bereitstellen. Beide Ausgaben referenzieren EPG-Daten desselben Targets. Unterschiedliche ID-Normalisierungen pro Ausgabe würden zwei Identitätsräume erzeugen und könnten zu inkonsistenten M3U-, Xtream-, XMLTV- und Short-EPG-Ergebnissen führen.

Deshalb gilt für ein Target genau eine EPG-ID-Kanonisierungsstrategie.

## 6. Bestehende relevante Datenwege

### 6.1 Smart Match und ID-Matching

`backend/src/processing/processor/epg.rs`

- sammelt `epg_channel_id` aus Live-Kanälen;
- speichert Membership-Keys ASCII-lowercase;
- führt seit PR #783 case-insensitive ID-Prüfungen durch;
- kann per Smart Match eine fehlende oder nicht gefundene Playlist-ID durch eine EPG-ID ersetzen.

Die Output-Normalisierung wird **nach** diesem Schritt ausgeführt. Smart Match arbeitet weiterhin mit den originalen Input-/Guide-Werten.

### 6.2 Target-Verarbeitung

`backend/src/processing/processor/playlist.rs`

- führt Filter, Rename und Mapper aus;
- wendet EPG-Zuordnung an;
- erzeugt die finale Target-Playlist;
- ruft anschließend `persist_playlist(...)` auf.

### 6.3 Target-Persistenz

`backend/src/repository/playlist_repository.rs`

- weist virtuelle IDs zu;
- materialisiert Serieninformationen;
- iteriert über die Target-Ausgaben;
- konvertiert dieselbe finale Target-Playlist nach M3U beziehungsweise Xtream;
- schreibt danach die passende Target-EPG-DB.

Das ist der effizienteste Ort für eine einmalige Normalisierung der finalen `epg_channel_id`.

### 6.4 M3U- und Xtream-Konvertierung

`shared/src/model/playlist.rs`

- `From<&PlaylistItem> for M3uPlaylistItem` kopiert `header.epg_channel_id`;
- `From<&PlaylistItem> for XtreamPlaylistItem` kopiert `header.epg_channel_id`;
- `M3uPlaylistItem::to_m3u()` schreibt den bereits gespeicherten Wert direkt als `tvg-id`;
- Xtream-Dokumente verwenden den bereits gespeicherten Wert direkt als `epg_channel_id`.

Wenn die Target-Playlist vor diesen Konvertierungen normalisiert wird, sind keine Renderer-spezifischen Änderungen erforderlich.

### 6.5 EPG-DB und XMLTV

`backend/src/repository/epg_repository.rs`

- schreibt pro EPG-fähigem Target-Output eine B+Tree-EPG-Datenbank;
- verwendet aktuell die originale `EpgChannel.id` als B+Tree-Schlüssel;
- baut die Rename Map aktuell aus der Playlist-`epg_channel_id`.

`backend/src/api/endpoints/xmltv_api.rs`

- schreibt `<channel id>` aus `EpgChannel.id`;
- schreibt `<programme channel>` ebenfalls aus `EpgChannel.id`;
- schreibt `<display-name>` aus `EpgChannel.title`;
- führt Short-EPG- und Stream-EPG-Abfragen über exakte DB-Schlüssel aus.

## 7. Globale Helper

### 7.1 Bestehenden Helper verschieben

Der bestehende Helper

```rust
with_folded_epg_id(...)
```

liegt derzeit in:

```text
backend/src/processing/processor/epg.rs
```

Er ist keine Processor-spezifische Logik mehr, sobald Repository- und API-Code dieselbe Kanonisierung verwenden. Er soll daher nach

```text
backend/src/utils/epg_id.rs
```

verschoben werden.

Damit vermeiden wir Abhängigkeiten der Repository-Schicht von der Processor-Schicht und behalten die Funktion dennoch ausschließlich im Backend. Ein Verschieben nach `shared` ist nicht erforderlich, weil M3U- und Xtream-Renderer nicht mehr selbst normalisieren.

### 7.2 Vorgeschlagene Helper

```rust
use shared::utils::Internable;
use std::{borrow::Cow, sync::Arc};

const EPG_ID_STACK_FOLD_LEN: usize = 128;

/// Führt `f` mit einer ASCII-lowercase EPG-ID aus.
/// Nicht-ASCII-Zeichen bleiben unverändert.
pub(crate) fn with_folded_epg_id<R>(id: &str, f: impl FnOnce(&str) -> R) -> R {
    if !id.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return f(id);
    }

    if id.len() <= EPG_ID_STACK_FOLD_LEN {
        let mut buf = [0_u8; EPG_ID_STACK_FOLD_LEN];
        for (out, byte) in buf.iter_mut().zip(id.bytes()) {
            *out = byte.to_ascii_lowercase();
        }
        let folded = std::str::from_utf8(&buf[..id.len()]);
        return f(folded.unwrap_or(id));
    }

    let mut folded = String::with_capacity(id.len());
    folded.extend(id.chars().map(|ch| ch.to_ascii_lowercase()));
    f(&folded)
}

/// Gibt eine owned/interned kanonische ID zurück.
/// Bei deaktivierter Normalisierung oder bereits kanonischem Wert wird nur der Arc geklont.
pub(crate) fn canonicalize_epg_id_arc(id: &Arc<str>, lowercase: bool) -> Arc<str> {
    if !lowercase {
        return Arc::clone(id);
    }

    with_folded_epg_id(id, |folded| {
        if folded == id.as_ref() {
            Arc::clone(id)
        } else {
            folded.intern()
        }
    })
}

/// Variante für Request- und DTO-Strings.
pub(crate) fn canonicalize_epg_id_str(id: &str, lowercase: bool) -> Arc<str> {
    if !lowercase {
        return id.intern();
    }

    with_folded_epg_id(id, |folded| folded.intern())
}

/// Unicode-Lowercase nur für sichtbaren XMLTV-Text.
/// Vermeidet eine Allokation, wenn die Option aus ist oder kein Großbuchstabe vorkommt.
pub(crate) fn lowercase_xmltv_text(value: &str, enabled: bool) -> Cow<'_, str> {
    if !enabled || !value.chars().any(char::is_uppercase) {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(value.to_lowercase())
    }
}
```

### 7.3 Wichtige Lifetime-Regel

Ein Helper auf Basis von `with_folded_epg_id` darf keinen geliehenen `&str` oder `Cow::Borrowed` aus dem Closure zurückgeben. Der gefaltete Wert kann auf einem lokalen Stack-Buffer oder einem lokalen `String` liegen.

Deshalb geben die ID-Helper ausschließlich ein owned/interned `Arc<str>` zurück. Nur `lowercase_xmltv_text`, das direkt vom Eingabe-`&str` leiht, darf `Cow<'_, str>` verwenden.

### 7.4 Modul-Export

`backend/src/utils/mod.rs`:

```rust
mod epg_id;

pub(crate) use self::epg_id::*;
```

Anschließend werden die bisherigen Imports in folgenden Dateien angepasst:

- `backend/src/processing/processor/epg.rs`;
- `backend/src/processing/parser/xmltv.rs`.

## 8. Konfigurationsmodell

### Datei

```text
shared/src/model/config/target.rs
```

### 8.1 Neue Struktur

```rust
#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EpgOutputOptions {
    #[serde(default, skip_serializing_if = "is_false")]
    pub lowercase_ids: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub lowercase_xmltv_display_names: bool,
}

impl EpgOutputOptions {
    pub const fn is_empty(&self) -> bool {
        !self.lowercase_ids && !self.lowercase_xmltv_display_names
    }
}
```

### 8.2 `ConfigTargetOptions` erweitern

```rust
#[serde(default, skip_serializing_if = "EpgOutputOptions::is_empty")]
pub epg_output: EpgOutputOptions,
```

`ConfigTargetOptions::is_empty()` wird ergänzt:

```rust
pub fn is_empty(&self) -> bool {
    !self.ignore_logo
        && self.share_live_streams.is_empty()
        && !self.remove_duplicates
        && self.epg_output.is_empty()
        && (self.force_redirect.is_none()
            || self.force_redirect.is_some_and(|f| f.has_full_flags() || f.is_empty()))
}
```

### 8.3 Zugriffsmethoden

```rust
impl ConfigTargetOptions {
    pub const fn lowercase_epg_ids(&self) -> bool {
        self.epg_output.lowercase_ids
    }

    pub const fn lowercase_xmltv_display_names(&self) -> bool {
        self.epg_output.lowercase_xmltv_display_names
    }
}
```

Die vorhandene `deny_unknown_fields`-Validierung bleibt erhalten. Unbekannte oder falsch geschriebene Felder werden weiterhin abgelehnt.

## 9. Einmalige Normalisierung der Target-Playlist

### Datei

```text
backend/src/repository/playlist_repository.rs
```

### 9.1 Neuer Helper

```rust
fn normalize_target_playlist_epg_ids(playlist: &mut [PlaylistGroup], lowercase_ids: bool) {
    if !lowercase_ids {
        return;
    }

    for group in playlist {
        for channel in &mut group.channels {
            let Some(epg_id) = channel.header.epg_channel_id.as_ref() else {
                continue;
            };
            if epg_id.is_empty() {
                continue;
            }

            let canonical = canonicalize_epg_id_arc(epg_id, true);
            channel.header.epg_channel_id = Some(canonical);
        }
    }
}
```

### 9.2 Aufrufposition

Der Aufruf erfolgt in `persist_playlist(...)`:

1. nach der Zuweisung virtueller IDs;
2. nach der Serien-/Episode-Materialisierung;
3. vor der Schleife über `target.output`;
4. vor M3U-/Xtream-Konvertierung und vor `epg_write_for_target(...)`.

Sinngemäß:

```rust
let lowercase_ids = target
    .options
    .as_ref()
    .is_some_and(ConfigTargetOptions::lowercase_epg_ids);

normalize_target_playlist_epg_ids(playlist, lowercase_ids);

for output in &target.output {
    // bestehender Output-Pfad
}
```

### 9.3 Warum genau hier?

- Smart Match hat zu diesem Zeitpunkt bereits eine endgültige `epg_channel_id` zugewiesen.
- Die Input-Playlist und Input-Caches bleiben unverändert.
- Alle Ausgaben eines Targets sehen denselben kanonischen Wert.
- M3U und Xtream müssen nicht bei jedem Request erneut normalisieren.
- Output-spezifische Filter arbeiten weiterhin auf derselben finalen Target-Playlist.

## 10. M3U- und Xtream-Ausgabe

Für folgende Dateien sind **keine funktionalen Änderungen** erforderlich:

```text
shared/src/model/playlist.rs
shared/src/model/playlist_document.rs
backend/src/model/xtream.rs
backend/src/repository/m3u_playlist_iterator.rs
```

Begründung:

- `M3uPlaylistItem::from(&PlaylistItem)` übernimmt die bereits kanonische ID;
- `XtreamPlaylistItem::from(&PlaylistItem)` übernimmt die bereits kanonische ID;
- `M3uPlaylistItem::to_m3u()` schreibt diese ID direkt;
- Xtream Live-Dokumente schreiben diese ID direkt.

Dies reduziert Code-Duplikation und verhindert, dass persistierte und dynamisch ausgelieferte Playlists unterschiedliche IDs erhalten.

## 11. Target-EPG-DB normalisieren

### Datei

```text
backend/src/repository/epg_repository.rs
```

### 11.1 Rename Map grundsätzlich case-insensitiv aufbauen

PR #783 behandelt EPG-IDs beim Matching case-insensitiv. Die Rename Map verwendet derzeit exakte Keys. Das kann dazu führen, dass eine Playlist-ID `rtlzwei.de` den EPG-Kanal `RTLzwei.de` zwar matcht, aber dessen Titel nicht überschreibt.

Die Rename Map soll deshalb unabhängig von der Output-Option mit ASCII-gefalteten Lookup-Keys arbeiten:

```rust
fn build_epg_rename_map(
    playlist: Option<&[PlaylistGroup]>,
) -> HashMap<Arc<str>, Arc<str>> {
    let mut rename_map = HashMap::new();

    if let Some(playlist) = playlist {
        for group in playlist {
            for channel in &group.channels {
                if let Some(epg_id) = &channel.header.epg_channel_id {
                    if !epg_id.is_empty() {
                        let lookup_key = canonicalize_epg_id_arc(epg_id, true);
                        rename_map.insert(lookup_key, Arc::clone(&channel.header.name));
                    }
                }
            }
        }
    }

    rename_map
}
```

Das ist eine kleine Korrektur der bestehenden PR-#783-Semantik und keine Änderung des sichtbaren Outputs, solange `lowercase_ids` deaktiviert ist.

### 11.2 `epg_write_file()` erweitern

Die Funktion erhält zusätzlich `lowercase_ids: bool` oder eine Referenz auf `EpgOutputOptions`.

```rust
pub fn epg_write_file<S: std::hash::BuildHasher>(
    target_name: &str,
    epg: &Epg,
    path: &Path,
    rename_map: &HashMap<Arc<str>, Arc<str>, S>,
    lowercase_ids: bool,
) -> Result<(), TuliproxError> {
    if epg.children.is_empty() {
        return Ok(());
    }

    let mut tree = BPlusTree::<Arc<str>, EpgChannel>::new();

    for channel in &epg.children {
        if channel.programmes.is_empty() {
            continue;
        }

        let mut chan = (**channel).clone();

        let rename_key = canonicalize_epg_id_arc(&chan.id, true);
        if let Some(title) = rename_map.get(&rename_key) {
            chan.title = Some(Arc::clone(title));
        }

        let output_id = canonicalize_epg_id_arc(&chan.id, lowercase_ids);
        chan.id = Arc::clone(&output_id);
        chan.programmes.sort_by_key(|programme| programme.start);

        tree.insert(output_id, chan);
    }

    tree.store(path).map_err(|err| {
        TuliproxError::RepositoryEpg(format!(
            "Failed to write epg for target {}: {} - {err}",
            target_name,
            path.display()
        ))
    })?;

    Ok(())
}
```

### 11.3 Warum B+Tree-Key und `EpgChannel.id` gemeinsam geändert werden müssen

Nur die XML-Ausgabe zu lowercasen wäre inkonsistent:

```text
XMLTV-ID:       rtlzwei.de
EPG-DB-Key:     RTLzwei.de
API-Query-ID:   rtlzwei.de
```

Dann könnten Short-EPG- und Stream-EPG-Abfragen den Kanal nicht finden. Deshalb werden sowohl der DB-Key als auch `EpgChannel.id` aus derselben kanonischen ID erzeugt.

### 11.4 Programme-Channel

`EpgProgramme.channel` ist transient und wird nicht als stabiler DB-Schlüssel verwendet. Der XMLTV-Writer setzt das `programme channel`-Attribut bereits aus `EpgChannel.id`. Deshalb ist keine Mutation jedes einzelnen Programms erforderlich.

Das vermeidet eine zusätzliche O(Anzahl Programme)-Normalisierung und spart erhebliche Arbeit bei großen EPGs.

### 11.5 `epg_write_for_target()`

Die Option wird einmal ermittelt und an beide EPG-fähigen Output-Pfade weitergegeben:

```rust
let lowercase_ids = target
    .options
    .as_ref()
    .is_some_and(ConfigTargetOptions::lowercase_epg_ids);
```

Anschließend wird `lowercase_ids` an `epg_write_file(...)` für M3U und Xtream übergeben.

## 12. XMLTV-Serialisierung

### Datei

```text
backend/src/api/endpoints/xmltv_api.rs
```

### 12.1 IDs

Keine zusätzliche ID-Transformation im XML-Writer:

```rust
elem.push_attribute((EPG_ATTRIB_ID, channel.id.as_ref()));
...
elem.push_attribute(("channel", channel.id.as_ref()));
```

`channel.id` stammt bereits kanonisch aus der Target-EPG-DB. Damit verwenden `<channel id>` und `<programme channel>` garantiert denselben Wert.

### 12.2 XMLTV-`display-name`

Die Option wird vor dem Writer-Task als `bool` ermittelt:

```rust
let lowercase_display_names = target
    .options
    .as_ref()
    .is_some_and(ConfigTargetOptions::lowercase_xmltv_display_names);
```

Beim Schreiben:

```rust
let title = channel.title.as_deref().unwrap_or("");
let display_name = lowercase_xmltv_text(title, lowercase_display_names);

continue_on_err!(
    writer
        .write_event_async(Event::Text(BytesText::new(display_name.as_ref())))
        .await
);
```

Der Titel in der EPG-DB bleibt unverändert. Damit beeinflusst die Option weder Web-UI-Daten noch andere JSON-Repräsentationen, sondern ausschließlich XMLTV-`display-name`.

Programmtitel und Programmbeschreibungen werden nicht verändert.

## 13. Runtime-EPG-Abfragen

Die B+Tree-Abfragen sind exakt. Deshalb werden eingehende IDs bei aktivierter Option vor dem Lookup kanonisiert.

### 13.1 Short EPG

In `serve_short_epg(...)`:

```rust
let lowercase_ids = target
    .options
    .as_ref()
    .is_some_and(ConfigTargetOptions::lowercase_epg_ids);

let query_channel_id = canonicalize_epg_id_arc(channel_id, lowercase_ids);

if let Some(epg_channel) = get_epg_channel(
    app_state,
    &query_channel_id,
    epg_path,
).await {
    // query_channel_id auch für epg_id/channel_id in der Antwort verwenden
}
```

Dadurch funktionieren sowohl bereits kanonische als auch abweichend geschriebene Client-Anfragen.

### 13.2 Stream EPG

In `serve_stream_epg(...)` werden Request-IDs vor folgenden Schritten kanonisiert:

1. Deduplizierung;
2. Aufbau der Query-ID-Liste;
3. Aufbau der `reference_ts`-Map;
4. Erzeugung der Response-`epg_channel_id`.

Empfohlene Datenstrukturen:

```rust
let mut seen = HashSet::<Arc<str>>::new();
let mut unique_ids = Vec::<Arc<str>>::new();
let mut reference_by_channel = HashMap::<Arc<str>, i64>::new();

for item in &items {
    let id = canonicalize_epg_id_str(&item.epg_channel_id, lowercase_ids);

    if seen.insert(Arc::clone(&id)) {
        unique_ids.push(Arc::clone(&id));
    }

    if let Some(reference_ts) = item.reference_ts {
        reference_by_channel.entry(id).or_insert(reference_ts);
    }
}
```

Die bestehende `epg_query_channels(...)`-Funktion muss nicht verändert werden. Sie erhält bereits die kanonischen B+Tree-Schlüssel.

## 14. Frontend

### Datei

```text
frontend/src/app/components/source_editor/target_form.rs
```

### 14.1 Reducer Actions

Ergänzen:

```rust
LowercaseEpgIds(bool),
LowercaseXmltvDisplayNames(bool),
```

Reducer:

```rust
ConfigTargetOptionsFormAction::LowercaseEpgIds(value) => {
    form.epg_output.lowercase_ids = value;
    modified = true;
}
ConfigTargetOptionsFormAction::LowercaseXmltvDisplayNames(value) => {
    form.epg_output.lowercase_xmltv_display_names = value;
    modified = true;
}
```

### 14.2 Darstellung

Unter Target → Options wird eine eigene Gruppe „EPG output“ ergänzt:

- „Lowercase EPG IDs“;
- „Lowercase XMLTV display names“.

Die read-only- und editierbare Ansicht müssen dieselben Felder darstellen.

### 14.3 Übersetzungen

Neue Labels werden in sämtlichen vom Frontend-Manifest geladenen Sprachdateien ergänzt, beispielsweise:

```text
LABEL.EPG_OUTPUT
LABEL.LOWERCASE_EPG_IDS
LABEL.LOWERCASE_XMLTV_DISPLAY_NAMES
```

## 15. Dokumentation und Changelog

### 15.1 Dokumentation

Datei:

```text
docs/src/configuration/source.md
```

Abschnitt:

```text
3.2.6 options
```

Ergänzen:

```yaml
options:
  epg_output:
    lowercase_ids: true
    lowercase_xmltv_display_names: false
```

In der Parametertabelle sind Wirkung, Default und der erforderliche Target-Neuaufbau zu dokumentieren.

### 15.2 Changelog

Datei:

```text
CHANGELOG.md
```

Hinweis:

- neue optionale Target-EPG-Output-Kanonisierung;
- Standard bleibt `preserve` beziehungsweise `false`;
- nach Aktivierung ist ein vollständiger Target-Refresh erforderlich;
- Clients können EPG-Zuordnungen nach dem ID-Wechsel einmalig neu aufbauen müssen.

## 16. Effizienzbewertung

### 16.1 Zeitkomplexität

| Schritt | Komplexität | Häufigkeit |
|---|---:|---|
| Target-Playlist-ID-Normalisierung | O(Anzahl Target-Einträge) | einmal je Target-Refresh |
| EPG-Channel-ID-Normalisierung | O(Anzahl übernommener EPG-Kanäle) | einmal je EPG-DB-Aufbau |
| XMLTV-Anzeigename | O(Länge des Namens) | nur beim XMLTV-Request und nur bei aktivierter Option |
| Short-/Stream-EPG-ID | O(Länge der Request-ID) | je angefragter ID |

Es erfolgt ausdrücklich keine Normalisierung jedes Programms und keine ID-Normalisierung bei jedem M3U-/Xtream-Rendering.

### 16.2 Allokationen

- bereits lowercase IDs verwenden weiterhin denselben interned `Arc<str>`;
- nur IDs mit ASCII-Großbuchstaben erzeugen einen neuen interned Wert;
- XMLTV-Anzeigenamen allokieren nur bei aktivierter Option und tatsächlich vorhandenen Unicode-Großbuchstaben;
- keine temporären vollständigen XML-Dokumente;
- keine zusätzliche Kopie der gesamten Playlist.

### 16.3 Interner

Die einmalige Target-Normalisierung ist besser als Renderer-Normalisierung, weil ein kanonischer String höchstens beim Target-Aufbau interned wird. Wiederholte Playlist-Downloads benötigen keine erneuten Interner-Lookups für die ID-Normalisierung.

### 16.4 Case-Kollisionen

ASCII-IDs, die sich ausschließlich in der Groß-/Kleinschreibung unterscheiden, werden durch PR #783 bereits als dieselbe EPG-Identität behandelt und im EPG-Merge unter einem gefalteten Schlüssel zusammengeführt.

Die neue Option führt daher keine neue Matching-Äquivalenz ein, sondern materialisiert dieselbe Äquivalenz im Output. Eine zusätzliche Kollisions-HashMap im Hot Path ist nicht erforderlich. Optional kann bei Debug-Logging eine begrenzte Diagnose ergänzt werden, sie ist jedoch kein Bestandteil der Kernimplementierung.

## 17. Sicherheits- und Korrektheitsbewertung

### 17.1 Keine Raw-XML-Manipulation

Die Normalisierung erfolgt auf typisierten Rust-Feldern. `quick_xml` übernimmt weiterhin das korrekte Escaping von Attributen und Text. Dadurch entstehen keine neuen XML-Injection-Pfade.

### 17.2 Keine konfigurierbaren Regexe

Die Funktion verwendet ausschließlich feste Case-Transformationen. Es gibt kein Regex- oder Template-Eingabefeld und damit kein zusätzliches ReDoS- oder Pattern-Injection-Risiko.

### 17.3 Deterministische ID-Semantik

EPG-IDs werden ausschließlich mit ASCII-Lowercase behandelt. Das entspricht PR #783 und verhindert locale-abhängiges Verhalten wie Turkish-I-Sonderfälle. Nicht-ASCII-Zeichen in IDs bleiben unverändert.

### 17.4 Unicode nur für sichtbaren Text

XMLTV-`display-name` verwendet Rusts Unicode-`to_lowercase()`. Eine mögliche Zeichenexpansion ist für sichtbaren Text zulässig, darf aber nicht für technische IDs verwendet werden.

### 17.5 Keine Mutation der Inputs

Die Normalisierung findet erst auf der fertig verarbeiteten Target-Playlist statt. Providerdaten, Input-Cache, Mapper-Eingangswerte und Smart-Match-Daten bleiben unverändert.

### 17.6 Konsistente Persistenz

Playlist-Ausgaben und EPG-DB werden innerhalb desselben Target-Verarbeitungslaufs aus derselben kanonisierten Target-Playlist erzeugt. Die bestehende Fehler- und Locking-Semantik wird nicht verändert.

Es wird keine neue Cross-File-Transaktion versprochen: Schlägt ein bestehender Output-Schreibpfad fehl, gilt weiterhin das bisherige Fehlerverhalten. Die Änderung darf diesen Zustand nicht durch zusätzliche unabhängige Renderer-Normalisierungen verschärfen.

### 17.7 Rückwärtskompatibilität

- beide Optionen sind standardmäßig `false`;
- bestehende Konfigurationen serialisieren unverändert;
- Parser bewahren weiterhin die Originalschreibweise;
- Smart Match bleibt unverändert;
- die Ausgabe ändert sich nur nach expliziter Aktivierung.

## 18. Migration und Betrieb

Nach Änderung einer der Optionen muss das Target vollständig neu aufgebaut werden, damit folgende Artefakte denselben Stand besitzen:

- M3U-/Xtream-Playlist-DB;
- persistierte M3U-Datei;
- Target-EPG-DB;
- Memory Cache, falls aktiviert.

Nach Aktivierung von `lowercase_ids` können Clients ihre EPG-Zuordnung einmalig neu indizieren müssen, weil sich sichtbare `tvg-id`-/`epg_channel_id`-Werte ändern.

Ein einfaches Config-Hot-Reload ohne Target-Refresh reicht nicht aus, solange die persistierten Artefakte noch die alte Schreibweise enthalten.

## 19. Testplan

### 19.1 Konfigurationsmodell

Datei:

```text
shared/src/model/config/target.rs
```

Tests:

1. fehlender `epg_output`-Block ergibt beide Werte `false`;
2. YAML-Roundtrip mit beiden Optionen;
3. `ConfigTargetOptions::is_empty()` berücksichtigt den neuen Block;
4. unbekannte Felder werden wegen `deny_unknown_fields` abgelehnt.

### 19.2 Helper

Datei:

```text
backend/src/utils/epg_id.rs
```

Tests:

1. `RTLzwei.de` → `rtlzwei.de`;
2. bereits lowercase nutzt denselben `Arc`;
3. IDs über 128 Bytes;
4. nicht-ASCII ID-Zeichen bleiben bei ASCII-Folding unverändert;
5. Unicode-Displayname `ÖSTERREICH` → `österreich`;
6. deaktivierte Optionen verändern nichts.

### 19.3 Target-Playlist

Datei:

```text
backend/src/repository/playlist_repository.rs
```

Tests:

1. finale Live-`epg_channel_id` wird lowercase;
2. leere und fehlende IDs bleiben unverändert;
3. `name`, `title`, `group` bleiben unverändert;
4. M3U- und Xtream-Konvertierungen erhalten denselben kanonischen Wert;
5. deaktivierte Option bewahrt MixedCase.

### 19.4 EPG-DB

Datei:

```text
backend/src/repository/epg_repository.rs
```

Tests:

1. DB-Key und `EpgChannel.id` sind bei aktiver Option lowercase;
2. deaktivierte Option bewahrt die Guide-Schreibweise;
3. Rename Map matcht `rtlzwei.de` zu `RTLzwei.de` auch ohne Output-Lowercase;
4. Programme bleiben erhalten und sortiert;
5. mehrere EPG-Quellen behalten ihre bestehende Prioritätssemantik.

### 19.5 XMLTV

Datei:

```text
backend/src/api/endpoints/xmltv_api.rs
```

Tests:

1. `<channel id>` ist lowercase;
2. `<programme channel>` ist exakt identisch zur Channel-ID;
3. `<display-name>` ist bei aktivierter Option Unicode-lowercase;
4. Programmtitel und Beschreibung bleiben unverändert;
5. bei deaktivierter Option bleibt die Originalschreibweise erhalten;
6. XML-Sonderzeichen werden weiterhin korrekt escaped.

### 19.6 Runtime Queries

Tests:

1. Short-EPG-Anfrage mit `RTLzwei.de` findet den DB-Key `rtlzwei.de` bei aktiver Option;
2. Stream-EPG-Anfragen werden vor Deduplizierung kanonisiert;
3. `reference_ts` bleibt der kanonischen ID korrekt zugeordnet;
4. Response-ID ist kanonisch;
5. deaktivierte Option behält die bisherige exakte Query-Semantik.

### 19.7 Regression PR #783

Bestehende Tests für folgende Eigenschaften müssen erhalten bleiben:

- case-insensitives ASCII-Matching;
- originale Parser-Schreibweise;
- case-insensitive Zuordnung von XMLTV-Programmen;
- unveränderte Ausgabe, wenn die neue Option deaktiviert ist.

## 20. Akzeptanzkriterien

Die Implementierung gilt als abgeschlossen, wenn:

1. bestehende Configs ohne neue Optionen identische Ausgaben erzeugen;
2. `lowercase_ids: true` M3U, Xtream, EPG-DB und XMLTV konsistent lowercased;
3. `<channel id>` und `<programme channel>` byte-identisch sind;
4. Short-/Stream-EPG dieselben kanonischen IDs verwenden;
5. `lowercase_xmltv_display_names` keine Playlist-Namen oder Programmtitel verändert;
6. keine Normalisierung in M3U-/Xtream-Request-Hot-Paths erforderlich ist;
7. `cargo fmt --all -- --check` erfolgreich ist;
8. `cargo clippy --all-targets --all-features -- -D warnings` für die betroffenen Crates erfolgreich ist;
9. Backend-, Shared- und Frontend-Tests erfolgreich sind;
10. Dokumentation und Changelog aktualisiert sind.

## 21. Empfohlene Implementierungsreihenfolge

1. `EpgOutputOptions` und Serde-Tests ergänzen.
2. `with_folded_epg_id` nach `backend/src/utils/epg_id.rs` verschieben und Helper-Tests ergänzen.
3. einmalige Target-Playlist-ID-Normalisierung in `persist_playlist()` implementieren.
4. EPG-DB-Key, `EpgChannel.id` und Rename-Map-Lookup anpassen.
5. XMLTV-`display-name`-Serialisierung ergänzen.
6. Short-/Stream-EPG-Query-Kanonisierung ergänzen.
7. Frontend-Toggles und Übersetzungen ergänzen.
8. Dokumentation und Changelog aktualisieren.
9. vollständige Regressionstests ausführen.

## 22. Zusammenfassung der betroffenen Dateien

### Neu

```text
backend/src/utils/epg_id.rs
```

### Ändern

```text
shared/src/model/config/target.rs
backend/src/utils/mod.rs
backend/src/processing/processor/epg.rs
backend/src/processing/parser/xmltv.rs
backend/src/repository/playlist_repository.rs
backend/src/repository/epg_repository.rs
backend/src/api/endpoints/xmltv_api.rs
frontend/src/app/components/source_editor/target_form.rs
docs/src/configuration/source.md
CHANGELOG.md
```

### Bewusst nicht ändern

```text
shared/src/model/playlist.rs
shared/src/model/playlist_document.rs
backend/src/model/xtream.rs
backend/src/repository/m3u_playlist_iterator.rs
backend/src/processing/parser/m3u.rs
```

Damit bleibt die Implementierung zentral, effizient und konsistent mit PR #783: Parser erhalten die Quellschreibweise, Matching bleibt ASCII-case-insensitiv und die kanonische Lowercase-Ausgabe wird ausschließlich durch eine explizite Target-Option aktiviert.
