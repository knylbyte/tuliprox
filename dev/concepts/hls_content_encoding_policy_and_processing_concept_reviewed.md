# HLS Content-Encoding: IST-Zustand und überarbeitetes Zielkonzept

Die Tests bestätigen die Ursache eindeutig. Im aktuellen Stand handelt es sich allerdings nicht nur um einen Fehler beim Shared-HLS-Manifest-Refresh,
sondern um ein allgemeines, bislang uneinheitliches Handling von HTTP-`Content-Encoding` in Legacy- und Shared-HLS.

Aktuell existieren im Wesentlichen vier unterschiedliche Verhaltensweisen:

1. Legacy-Manifeste werden teilweise dekomprimiert.
2. Shared-Manifeste werden überhaupt nicht dekomprimiert.
3. Shared-Segmente, Maps und Keys fordern zwar `identity` an, besitzen aber keinen Decoder-Fallback.
4. Legacy-Segmente, Maps und Keys werden codiert weitergeleitet, während `Content-Encoding` verloren geht.

Grundlage der Analyse ist der bereitgestellte Develop-Stand **3.3.61**. Die genannten Zeilennummern und Funktionspositionen beziehen sich auf diesen
Stand; bei späteren Änderungen sind die symbolischen Namen und Repository-Pfade maßgeblich.

> **Überarbeitungsstand 2026-07-13:** Diese Fassung integriert die Codequalitäts-, Effizienz- und DRY-Prüfung.
> Präzisiert wurden insbesondere das finale Request-Enforcement, die schlankere Decoder-API, die Zuständigkeitsgrenzen
> für Range, Retry und Timeout, die sichere Header-Auswahl sowie die phasenweise Umsetzung gemäß `AGENTS.md`.

---

## Schritt 1: Aktueller IST-Zustand

### 1. Vorhandene globale Kompressionsunterstützung

Tuliprox verwendet derzeit:

```toml
reqwest = {
    version = "0.13.4",
    features = ["json", "stream", "rustls", "socks", "form"]
}

async-compression = {
    version = "0.4.42",
    features = ["tokio", "gzip", "zlib"]
}
```

`reqwest` besitzt damit keine aktivierte automatische Dekompression für gzip, Deflate, Brotli oder Zstandard. Die Dekompression erfolgt teilweise
selbst über `async-compression`.

Relevante Datei:

```text
backend/Cargo.toml
```

In `backend/src/utils/network/request.rs` existiert bereits:

```rust
async fn build_decoded_stream_reader(
    response: reqwest::Response,
) -> Result<DynReader, std::io::Error>
```

Dieser Helper:

- liest `Content-Encoding`,
- erkennt gzip zusätzlich über `1f 8b`,
- erkennt einen zlib-Header über `78 01`, `78 9c` oder `78 da`,
- verwendet `GzipDecoder`,
- verwendet für `deflate` ausschließlich `ZlibDecoder`,
- reicht unbekannte Codierungen unverändert weiter.

Er unterstützt nicht:

- Brotli (`br`),
- Zstandard (`zstd`),
- echtes Raw-Deflate,
- mehrere hintereinander angewandte Codierungen,
- mehrere `Content-Encoding`-Header,
- `x-gzip`,
- strukturierte Fehlerklassen,
- ein Limit auf die dekomprimierte Größe,
- eine korrekte Normalisierung der Response-Header nach der Dekompression.

Zusätzlich ist `is_deflate` irreführend benannt. Der Helper erkennt nicht allgemein Deflate, sondern nur einige typische **zlib-Wrapper-Header**:

```rust
pub const fn is_deflate(bytes: &[u8]) -> bool {
    bytes[0] == 0x78
        && (bytes[1] == 0x01
            || bytes[1] == 0x9C
            || bytes[1] == 0xDA)
}
```

Relevante Datei:

```text
backend/src/utils/compression/compression_utils.rs
```

---

### 2. IST-Matrix

| Pfad | Origin-Request | Verarbeitung des Origin-Bodys | Ausgabe beziehungsweise Cache | Problem |
| --- | --- | --- | --- | --- |
| Legacy Manifest | Client-/Input-Header werden übernommen; `Range` wird entfernt; `Accept-Encoding` bleibt erhalten | gzip und zlib teilweise unterstützt | Manifest wird als String neu geschrieben | `br`, `zstd`, Raw-Deflate und mehrere Codierungen schlagen fehl |
| Legacy Segment/Map/Key | Client-`Accept-Encoding` kann zum Provider gelangen | Keine Dekompression | Codierter Body wird gestreamt, `Content-Encoding` wird verworfen | Client erhält komprimierte Bytes ohne Kennzeichnung |
| Shared Manifest | Gleiches Manifest-Header-Building; kein erzwungenes `identity` | Rohe Bytes werden direkt als UTF-8 interpretiert | Danach erst Parsing/Cache-Timeline | Bestätigter Fehler `origin manifest is not UTF-8` |
| Shared Segment | `Accept-Encoding: identity` wird gesetzt | Keine Fallback-Dekompression | Origin-Bytes gehen direkt in Repair und Cache | Ignoriert Provider `identity`, wird ein komprimiertes Segment gecacht |
| Shared Map | `Accept-Encoding: identity` wird gesetzt | Keine Fallback-Dekompression | Origin-Bytes gehen direkt in Cache | fMP4-Init-Map kann als gzip/br/zstd-Wrapper gecacht werden |
| Shared Key | `Accept-Encoding: identity` wird gesetzt | Keine Fallback-Dekompression | Raw-Passthrough, Key wird nicht gecacht | Komprimierte Key-Bytes werden ohne `Content-Encoding` ausgeliefert |
| Shared Transient Resource | `Accept-Encoding: identity` wird gesetzt | Keine Fallback-Dekompression | Je nach Request Cache oder Passthrough | Gleiche Probleme für Parts, Maps, Keys und sonstige URIs |

---

## 3. Legacy-HLS im Detail

### 3.1 Legacy-Manifest

`build_hls_manifest_request_headers` entfernt einen vom Client gelieferten `Range`-Header, setzt aber kein `Accept-Encoding: identity`.

Wichtiger ist, dass dieser Helper nicht die letzte Stelle vor dem Origin-Request ist. Der Legacy-Pfad erzeugt anschließend erneut ein
`InputSource`; `prepare_input_request_headers` merged dessen konfigurierte `input.headers` noch einmal mit den bereits vorbereiteten Headern.
Konfigurierte Input-Header besitzen dabei Priorität. Ein früher gesetztes `Accept-Encoding: identity` könnte deshalb später wieder durch
`gzip`, `br` oder einen anderen Wert ersetzt werden.

Die HLS-Policy muss folglich am finalen Request-Boundary gelten: nach allen Header-Merges und unmittelbar bevor der konkrete
`reqwest::RequestBuilder` pro Versuch beziehungsweise Redirect-Hopf erzeugt oder abgesendet wird. Der HLS-Header-Builder bleibt trotzdem für
Sanitizing und das Entfernen eines unkontrollierten `Range`-Headers zuständig.

Ohne dieses finale Enforcement können sowohl ein Input-Header als auch ein Client-Header wie dieser am Origin ankommen:

```http
Accept-Encoding: gzip, deflate, br, zstd
```

Relevante Datei:

```text
backend/src/api/endpoints/hls_api.rs
```

Der Legacy-Manifestpfad verwendet anschließend:

```rust
download_text_content_with_headers(...)
```

beziehungsweise:

```rust
download_text_content_with_manual_redirects_and_headers(...)
```

Diese Funktionen verwenden den vorhandenen `build_decoded_stream_reader`. gzip und zlib funktionieren deshalb teilweise bereits. Danach wird das
dekomprimierte Manifest über `rewrite_hls` umgeschrieben.

Relevante Dateien:

```text
backend/src/api/endpoints/hls_api.rs
backend/src/utils/network/request.rs
backend/src/processing/parser/hls.rs
```

Das Client-Manifest wird vollständig neu erzeugt. Die Origin-Codierung darf und muss deshalb nicht übernommen werden. Die globale
Tower-`CompressionLayer` kann das neu erzeugte Manifest anschließend abhängig von den Client-Fähigkeiten als gzip, Deflate, Brotli oder Zstandard
ausliefern.

Relevante Datei:

```text
backend/src/api/main_api.rs
```

#### Ergebnis

Legacy-Manifeste sind heute nur zufällig kompatibler als Shared-Manifeste. Die Implementierung ist trotzdem unvollständig:

- Kein `identity`-Request.
- Kein Brotli.
- Kein Zstandard.
- Deflate nur als zlib.
- Kein dekomprimiertes Größenlimit.
- Keine Behandlung mehrerer Codierungen.
- Ein unbekanntes `Content-Encoding` wird nicht abgelehnt, sondern als Text gelesen.

---

### 3.2 Legacy-Segmente, Maps und Keys

Der Legacy-Rewriter schreibt sowohl gewöhnliche Segmentzeilen als auch alle `URI="..."`-Attribute auf den Legacy-HLS-Endpunkt um. Damit laufen über
denselben Pfad unter anderem:

- MPEG-TS-Segmente,
- fMP4-Segmente,
- `EXT-X-MAP`,
- `EXT-X-KEY`,
- `EXT-X-PART`,
- weitere URI-basierte Ressourcen.

Relevante Datei:

```text
backend/src/processing/parser/hls.rs
```

Diese Requests gelangen danach in `force_provider_stream_response` und weiter in `provider_stream_request`.

Der Body wird dort unverändert übernommen:

```rust
let provider_stream = response
    .bytes_stream()
    .map_err(|err| StreamError::reqwest(&err))
    .boxed();
```

Relevante Datei:

```text
backend/src/api/model/streams/provider_stream_factory.rs
```

Die Response-Header werden dagegen über `get_response_headers` gefiltert. Die globale Allowlist enthält derzeit:

```text
content-type
content-length
content-range
vary
transfer-encoding
...
```

aber **kein**:

```text
content-encoding
```

Relevante Dateien:

```text
backend/src/api/model/model_utils.rs
shared/src/utils/constants.rs
```

Damit passiert bei einer gzip-Antwort aktuell:

```text
Origin:
Content-Encoding: gzip
Content-Length: 1234
Body: gzip-codierte Segmentbytes

Tuliprox:
Content-Encoding wird entfernt
Content-Length bleibt 1234
Body bleibt gzip-codiert

Client:
interpretiert gzip-Container als MPEG-TS/fMP4/Key
```

Da die Legacy-Streaming-Response zusätzlich über `mark_response_as_uncompressed` von der globalen Response-Kompression ausgeschlossen wird, erfolgt
auch keine nachträgliche Korrektur.

Das ist für folgende Fälle unmittelbar defekt:

- Segment: Player sieht keinen MPEG-TS-Sync-Byte- beziehungsweise keinen MP4-Container.
- Map: Initialisierungssegment ist ungültig.
- Key: Der Player erhält nicht die eigentlichen Key-Bytes.
- Range: Bytepositionen beziehen sich auf die codierte statt auf die erwartete Repräsentation.

Nebenbei enthält die Allowlist momentan `Transfer-Encoding`. Dieser Header ist hop-by-hop und darf beim Neubau einer Axum-Response nicht vom Origin
übernommen werden.

---

## 4. Shared-HLS im Detail

### 4.1 Shared-Manifest

Der Shared-Manifestpfad liest den Body direkt aus `response.bytes_stream()` in einen `Vec<u8>` und führt anschließend aus:

```rust
String::from_utf8(body)
```

Es findet keinerlei Auswertung von `Content-Encoding` statt.

Der vorhandene Grenzwert von 2 MiB bezieht sich damit auf die **komprimierte Wire-Repräsentation**, nicht auf die dekomprimierte Manifestgröße.

Relevante Datei:

```text
backend/src/api/model/hls_cache/manifest_fetch.rs
```

Der Refresh-Kontext übernimmt die Origin-Header ohne verbindliche `identity`-Normalisierung:

```rust
headers: hls_origin_headers_with_provider_session(
    &request.headers,
    &request.origin_provider_session_headers,
)
```

Der initiale Shared-Manifest-Request läuft anschließend über den generischen Input-/Provider-Request-Stack, der Input-Header erneut zusammenführt.
Recovery-Requests verwenden dagegen direkte `reqwest`-Requests. Daher muss die Identity-Policy an beiden tatsächlichen Request-Boundaries gelten:

1. im generischen Request-Builder nach dem finalen Header-Merge und
2. in direkten Shared-HLS-Recovery-Requests unmittelbar vor `.send()`.

Relevante Dateien:

```text
backend/src/api/model/hls_cache/refresh.rs
backend/src/api/model/hls_cache/manifest_fetch.rs
backend/src/utils/network/request.rs
```

Das fehlende Response-Decoding erklärt den bestätigten UTF-8-Fehler; die fehlende finale Request-Policy erklärt, weshalb ein bloßes frühes Setzen des
Headers keine belastbare Gesamtlösung wäre.

---

### 4.2 Shared-Segmente und Maps

Für Shared-Segmente und Maps ist bereits ein sinnvoller Request-Ansatz vorhanden:

```rust
pub fn force_identity_without_range(headers: &mut HeaderMap) {
    headers.remove(header::RANGE);
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("identity"),
    );
}
```

Relevante Datei:

```text
backend/src/api/model/hls_cache/headers.rs
```

`build_hls_origin_resource_headers` verwendet diesen Helper und setzt danach gegebenenfalls einen von Tuliprox kontrollierten Byte-Range neu ein.

Relevante Datei:

```text
backend/src/api/model/hls_cache/resource_fetch.rs
```

Das ist richtig, solange der Provider die Vorgabe respektiert. Der Response-Body wird jedoch unverändert verarbeitet:

```rust
let stream_reader =
    StreamReader::new(response.bytes_stream().map_err(io::Error::other));
```

Das passiert sowohl beim Segment als auch bei der Map.

Relevante Dateien:

```text
backend/src/api/model/hls_cache/segment_fetcher.rs
backend/src/api/model/hls_cache/map_fetcher.rs
```

Ignoriert der Provider `identity`, erhält:

- Segment Repair einen gzip/br/zstd-Container statt eines Medienobjekts,
- ffmpeg ungültige Eingabedaten,
- der Cache die komprimierte Transportrepräsentation,
- die Cache-Metadaten lediglich deren Dateigröße,
- der Client später diese Bytes ohne `Content-Encoding`.

Die Shared-Cache-Antworten setzen selbst `Content-Type`, `Content-Length`, `Content-Range` und `Accept-Ranges` und markieren den Body als nicht durch
Tower zu komprimieren.

Relevante Datei:

```text
backend/src/api/model/hls_cache/response.rs
```

Der Shared-Cache besitzt also implizit bereits die Annahme:

> Jede Cachedatei enthält die kanonische Identity-Repräsentation des HLS-Objekts.

Diese Invariante wird beim Schreiben bisher jedoch nicht erzwungen.

---

### 4.3 Shared-Key und Transient-Modus

Ein `EXT-X-KEY` führt im normalen Shared-Parser bewusst zum Transient-Passthrough-Modus.

Relevante Datei:

```text
backend/src/processing/parser/hls/origin_manifest.rs
```

Der Transient-Rewriter kategorisiert Ressourcen als:

```rust
pub enum TransientResourceKind {
    Segment,
    Key,
    Map,
    Other,
}
```

und schreibt Key-, Map- sowie sonstige URI-Attribute auf den Shared-Resource-Endpunkt um.

Relevante Datei:

```text
backend/src/processing/parser/hls/transient_manifest.rs
```

Keys werden absichtlich nicht gecacht:

```rust
if matches!(resource.kind, TransientResourceKind::Key) {
    return HlsTransientObjectCacheAction::PassthroughNoCache;
}
```

Der Passthrough-Code:

- streamt erneut `response.bytes_stream()` unverändert,
- übernimmt nur ausgewählte Header,
- übernimmt kein `Content-Encoding`,
- setzt derzeit kein `mark_response_as_uncompressed`.

Relevante Datei:

```text
backend/src/api/model/hls_cache/transient_fetcher.rs
```

Ein komprimierter Key würde daher als komprimierter Body ohne Codierungskennzeichnung zum Player gelangen. Das Resultat wäre typischerweise eine
falsche Key-Länge und anschließend ein Segment-Decryption-Fehler.

Wichtig ist die Trennung:

- `Content-Encoding` ist HTTP-Transportcodierung.
- `EXT-X-KEY` beziehungsweise AES-Verschlüsselung ist HLS-Inhaltsverschlüsselung.

Tuliprox muss nur die HTTP-Transportcodierung entfernen. Die HLS-Verschlüsselung der Segmentbytes bleibt vollständig erhalten.

---

---

## 5. Verbindliche Anforderungen aus den Findings

Aus dem IST-Zustand ergeben sich acht verbindliche Anforderungen. Sie definieren die fachlichen Invarianten; die konkrete Rust-API wird danach so
schmal wie möglich gewählt.

### 5.1 Origin-Repräsentation am finalen Request-Boundary festlegen

Für alle HLS-Origin-Requests fordert Tuliprox an:

```http
Accept-Encoding: identity
```

Das betrifft:

- Legacy-Manifest,
- Legacy-Segment/Map/Key/Part/Other,
- Shared-Manifest einschließlich Recovery-Requests,
- Shared-Segment/Map/Key/Part/Other.

Die Policy darf nicht nur in einem frühen HLS-Header-Builder gesetzt werden. Sie muss nach allen Merges aus:

- Input-Konfiguration,
- Client-Request,
- Default-Headern,
- Provider-/Session-Headern,
- Redirect-spezifischem Header-Scrubbing

unmittelbar vor dem Bau jedes realen `reqwest::Request` erneut angewandt werden. Dadurch kann weder ein Client- noch ein Input-Header die HLS-Policy
zurücküberschreiben.

Frühes Setzen in HLS-spezifischen Header-Buildern bleibt sinnvoll, weil es die Invariante dokumentiert und direkte Request-Pfade absichert. Es ersetzt
aber nicht das finale Enforcement.

### 5.2 Deklarierte Origin-Kompression als Fallback dekodieren

Provider können `Accept-Encoding: identity` ignorieren. Jeder HLS-Pfad muss deshalb deklarierte `Content-Encoding`-Antworten nach Identity dekodieren.
Unterstützt werden mindestens:

- `identity`,
- `gzip`,
- `x-gzip`,
- `deflate` mit Unterscheidung zwischen zlib-Wrapper und Raw-Deflate,
- `br`,
- `zstd`,
- mehrere Codierungen in der vom HTTP-Header angegebenen Reihenfolge und beim Decoding in umgekehrter Reihenfolge.

Unbekannte oder syntaktisch ungültige Codierungen werden explizit abgelehnt. Sie dürfen nicht als scheinbare Identity-Bytes an Parser, Cache oder
Client gelangen.

### 5.3 Kanonische Identity-Repräsentation

Nach dem Origin-Fetch arbeitet die HLS-Pipeline intern ausschließlich mit der dekodierten Identity-Repräsentation:

```text
Origin Content Representation
        ↓
HTTP Content Decoder
        ↓
Identity Representation
        ├── Manifest Parser / Rewriter
        ├── Segment Repair
        ├── Shared Cache
        └── HLS Client Response
```

Insbesondere gilt:

> Kein transportkomprimiertes Objekt darf im Shared-HLS-Cache gespeichert werden.

Dadurch bleiben bestehende Cache-Keys, Range-Indizes und Cache-Metadaten unverändert. Eine Erweiterung des Cache-Schemas um `Content-Encoding` ist
nicht erforderlich.

### 5.4 Response-Header nach einer Transformation korrigieren

Wurde ein Body dekomprimiert, beziehen sich repräsentationsabhängige Header nicht mehr auf den ausgelieferten Body. Nach einer Transformation nach
Identity werden daher entfernt oder neu berechnet:

- `Content-Encoding`: entfernen,
- `Content-Length`: entfernen oder nach vollständigem Puffern neu setzen,
- `Content-Range`: bei transformierter Vollantwort entfernen,
- `Accept-Ranges`: bei transformierter Vollantwort entfernen,
- `Transfer-Encoding`: nie in eine neu erzeugte Axum-Response übernehmen,
- `ETag`: nach Transformation entfernen,
- `Content-MD5`, `Digest`, `Content-Digest`, `Repr-Digest`: nach Transformation entfernen,
- `Vary`: nur den Token `Accept-Encoding` entfernen; andere Tokens erhalten.

Folgende Header können bei unveränderter Bedeutung grundsätzlich erhalten bleiben, werden aber weiterhin nur über eine explizite sichere
Client-Header-Auswahl weitergegeben:

- `Content-Type`,
- `Cache-Control`,
- `Expires`,
- `Last-Modified`.

Es wird **kein** generischer Helper eingeführt, der pauschal alle End-to-End-Header kopiert. Provider-Cookies, Authentifizierungsheader,
CORS-/CSP-Header oder proprietäre CDN-Header dürfen nicht unbeabsichtigt an Tuliprox-Clients gelangen.

### 5.5 Bestehende Range-Policy bleibt fachliche Quelle

Shared-HLS besitzt bereits:

```rust
HlsOriginByteRangeExpectation
```

Diese Struktur bleibt die alleinige fachliche Quelle dafür, ob `200 OK`, `206 Partial Content` oder ein bestimmtes `Content-Range` zulässig ist. Der
Content-Decoder führt keine zweite Range-Policy ein.

Der Decoder erzwingt nur eine protokollbedingte Sicherheitsregel:

```text
206 Partial Content + nicht-Identity Content-Encoding → ablehnen
```

Ein Ausschnitt kann mitten in einem gzip-, Brotli-, Zstandard- oder Deflate-Stream beginnen und ist nicht allgemein dekodierbar.

| Response | Zuständigkeit und Verhalten |
| --- | --- |
| `200` + keine Codierung | Decoder reicht Identity durch; Fachpfad bewertet den Status |
| `200` + unterstützte Codierung | Decoder normalisiert streamend nach Identity |
| `206` + keine Codierung/`identity` | Fachpfad bewertet Range; Decoder reicht Body durch |
| `206` + andere Codierung | Decoder lehnt als `EncodedPartialContent` ab |
| `200` auf einen Range-Request + Codierung | Fachpfad entscheidet, ob `200` zulässig ist; bei Zulässigkeit vollständig dekodierbar |

### 5.6 Magic-Sniffing nur für HLS-Manifeste

Die Priorität ist verbindlich:

1. Ein vorhandener, gültiger `Content-Encoding`-Header ist maßgeblich.
2. Dekodiert wird in umgekehrter Reihenfolge der deklarierten Codierungen.
3. Magic-Sniffing erfolgt ausschließlich, wenn `Content-Encoding` vollständig fehlt.
4. Magic-Sniffing wird nur im HLS-Manifestpfad aktiviert.
5. Binäre Ressourcen verwenden ausschließlich deklarierte Codierungen.
6. Ein deklarierter Header wird niemals anhand der Bodybytes stillschweigend überschrieben.

Für HLS-Manifeste dürfen bei fehlendem Header bekannte Signaturen erkannt werden:

```text
1f 8b       → gzip
78 xx       → möglicher zlib-Wrapper; vollständige zlib-Headerprüfung erforderlich
28 b5 2f fd → zstd
```

Brotli besitzt keine für diesen Zweck ausreichend belastbare kurze Magic-Signatur und wird ohne Header nicht geraten.

Für Segment, Map, Key, Part und sonstige Binärobjekte gilt:

> Fehlt `Content-Encoding`, ist der Body gemäß HTTP eine Identity-Repräsentation.

Ein binärer Key kann zufällig mit `1f 8b` beginnen und darf deshalb niemals aufgrund seiner ersten Bytes dekomprimiert werden.

Allgemeine M3U-, Xtream- oder andere Textdownloads verwenden standardmäßig ebenfalls nur deklarierte Codierungen. Die fehlertolerante
Manifest-Magic-Erkennung ist ein gezieltes HLS-Verhalten und kein globaler Default.

### 5.7 Deadline umfasst Decoder-Vorbereitung und Bodykonsum

Der Decoder kann vor dem eigentlichen Konsum Bytes benötigen, etwa für:

- HLS-Manifest-Magic-Erkennung,
- zlib-/Raw-Deflate-Unterscheidung,
- Initialisierung einer Decoderkette.

Diese Vorbereitung darf keine unbeschränkte Phase vor dem bestehenden Timeout eröffnen. Bei Manifesten müssen:

```text
Response-Decoding vorbereiten + gesamten dekodierten Body lesen + UTF-8 validieren
```

unter derselben Manifest-Deadline laufen.

Bei Segmenten, Maps und Transient-Objekten wird der Decoder innerhalb der bestehenden Object-/Body-/Idle-Deadline konsumiert. Es wird keine zweite,
unabhängige Timeout-Architektur eingeführt.

### 5.8 Retry-Grenzen sind pfadspezifisch

Decoderfehler sind nur dann transparent retryfähig, wenn der Body vor der Erfolgsrückgabe vollständig konsumiert wird.

| Pfad | Body vor Erfolgsrückgabe vollständig konsumiert? | Transparenter Retry bei Decoderfehler |
| --- | ---: | ---: |
| Legacy-Manifest | Ja | Ja |
| Shared-Manifest | Ja | Ja |
| Shared-Segment-Cachewrite | Ja | Ja |
| Shared-Map-Cachewrite | Ja | Ja |
| Shared-Transient mit Cachewrite | Ja | Ja |
| Shared-Key-Passthrough | Nein | Nach Beginn der Client-Response nein |
| Shared-Transient-Passthrough | Nein | Nach Beginn der Client-Response nein |
| Legacy-Segment/Map/Key-Streaming | Nein | Nach Beginn der Client-Response nein |

Für direkte Streams gilt deshalb:

- Fehler vor dem Aufbau der Client-Response können nach bestehender Policy retryfähig sein.
- Decoderfehler während des Client-Streamings beenden den Stream.
- Nach dem Senden der Response-Header darf kein transparenter Origin-Wechsel vorgetäuscht werden.

Ein vollständiges Puffern kleiner Keys kann später als separate, kompatibilitätsgeprüfte Optimierung eingeführt werden. Es ist keine Voraussetzung für
die korrekte streamende Dekompression und wird nicht als verstecktes neues Größenlimit in diesen Kernchange aufgenommen.

---

## Schritt 2: Überarbeitetes Zielkonzept

## 6. Zentrale Architekturentscheidung

Für HLS gilt folgende feste Policy:

> Tuliprox fordert am finalen Origin-Request-Boundary Identity an, dekodiert abweichend codierte vollständige Origin-Antworten als Fallback und
> verarbeitet, repariert, cached sowie streamt intern ausschließlich Identity-Bytes.

Origin- und Client-Kompression werden strikt getrennt.

### 6.1 Originseite

```text
HLS Header sammeln
    ↓
Input-/Client-/Provider-/Session-Header zusammenführen
    ↓
Sensitive/disabled Header entfernen
    ↓
kontrollierten Range setzen
    ↓
FINAL: Accept-Encoding = identity erzwingen
    ↓
Origin-Request senden
```

### 6.2 Eingehende Origin-Antwort

```text
Origin-Status und bestehende Range-Erwartung prüfen
    ↓
Content-Encoding parsen
    ↓
encoded 206 ablehnen
    ↓
unter bestehender Deadline streamend nach Identity dekodieren
    ↓
repräsentationsabhängige Header normalisieren
```

### 6.3 Clientseite

- **Manifest:** Tuliprox erzeugt neuen UTF-8-Text. Die bestehende Tower-`CompressionLayer` darf diesen entsprechend dem Client-`Accept-Encoding`
  komprimieren.
- **Segment, Map, Key, Part und andere Binärobjekte:** Tuliprox liefert Identity aus und markiert die Response mit
  `mark_response_as_uncompressed`, damit Tower keine erneute Codierung anwendet.
- **Nicht-HLS-Provider-Streams:** bleiben standardmäßig im Preserve-Modus; Body und `Content-Encoding` müssen gemeinsam erhalten bleiben.

Eine Cache-Variantenlösung nach Content-Coding wird ausdrücklich nicht verfolgt. Sie würde Cache-Keys, `Vary`-Semantik, Range-Indizes,
Repair-Pfade und Client-Negotiation unnötig vervielfachen.

---

## 7. Schlanker globaler Content-Coding-Layer

Empfohlener Ort:

```text
backend/src/utils/network/content_coding.rs
```

Das Modul bleibt crate-intern. Es ist kein generisches Proxy-Framework, sondern bündelt ausschließlich die wiederverwendbaren HTTP-
Content-Coding-Invarianten.

Der vorhandene Typ wird wiederverwendet:

```rust
pub type DynReader = Pin<Box<dyn AsyncRead + Send>>;
```

Ein zweiter Alias wie `DynHttpBodyReader` wird nicht eingeführt.

### 7.1 Outbound-Policy

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum OutboundContentCodingPolicy {
    #[default]
    Inherit,
    Identity,
}
```

Diese Policy steuert nur den ausgehenden `Accept-Encoding`-Header. Sie enthält keine Range-, Timeout- oder Response-Logik.

### 7.2 Content-Coding-Typen

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentCoding {
    Gzip,
    Deflate,
    Brotli,
    Zstd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentCodingDetection {
    /// Ausschließlich Content-Encoding auswerten.
    DeclaredOnly,

    /// Nur bei vollständig fehlendem Content-Encoding zusätzlich bekannte
    /// Signaturen eines erwarteten HLS-Textmanifests erkennen.
    DeclaredOrKnownHlsManifestMagic,
}
```

### 7.3 Dekodierte Response

```rust
pub(crate) struct DecodedHttpResponse {
    pub status: StatusCode,
    pub final_url: Url,

    /// Origin-Header, nach einer tatsächlichen Transformation auf die nun
    /// vorliegende Identity-Repräsentation normalisiert.
    /// Interne Header wie Set-Cookie dürfen enthalten bleiben; die Auswahl
    /// für den Client erfolgt separat über eine sichere Allowlist.
    pub headers: HeaderMap,

    pub body: DynReader,

    /// Leer bei unveränderter Identity-Repräsentation.
    pub decoded_from: Vec<ContentCoding>,
}
```

Es werden nicht zwei vollständige Header-Maps pro Response gehalten. Für interne Auswertung und spätere Client-Selektion reicht eine geklonte,
repräsentationskonsistente `HeaderMap`.

### 7.4 Setup-/Headerfehler

```rust
#[derive(Debug, thiserror::Error)]
pub(crate) enum ContentCodingError {
    #[error("invalid Content-Encoding header")]
    InvalidHeader,

    #[error("unsupported Content-Encoding: {0}")]
    Unsupported(String),

    #[error("encoded partial content cannot be decoded safely")]
    EncodedPartialContent,

    #[error("failed to inspect encoded response prefix: {0}")]
    PrefixRead(#[from] io::Error),
}
```

### 7.5 Fehler während des streamenden Lesens

```rust
#[derive(Debug, thiserror::Error)]
#[error("content decoding failed: coding={coding:?}, detail={detail}")]
pub(crate) struct ContentDecodingIoError {
    pub coding: ContentCoding,
    pub detail: String,
}
```

Decoder werden so gewrappt, dass ein späterer Fehler des jeweiligen `async-compression`-Readers diesen typisierten Fehler als Source eines
`io::Error` trägt. Dadurch kann Shared-HLS Decoderfehler von Cache-/Dateisystemfehlern unterscheiden, obwohl beide während eines streamenden
Cachewrites auftreten können.

### 7.6 Fehler beim vollständigen Konsum kleiner Textkörper

```rust
#[derive(Debug, thiserror::Error)]
pub(crate) enum ContentBodyReadError {
    #[error("decoded body exceeds configured limit {limit}")]
    LimitExceeded { limit: usize },

    #[error("decoded body is not valid UTF-8: valid_up_to={valid_up_to} error_len={error_len:?}")]
    InvalidUtf8 {
        valid_up_to: usize,
        error_len: Option<usize>,
    },

    #[error(transparent)]
    Io(#[from] io::Error),
}
```

Diese Fehlerklasse gehört zum Consumer-Layer für vollständig gelesene Bodies. Sie dupliziert nicht die Setup-Fehler des Content-Decoders.

---

## 8. Explizit zu verwendende Helper

## 8.1 Globale Helper in `utils/network/content_coding.rs`

### `force_accept_encoding_identity`

```rust
pub(crate) fn force_accept_encoding_identity(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("identity"),
    );
}
```

**Einsatz:** immer dann, wenn ein bereits vollständig gebauter `HeaderMap`-Satz verbindlich auf die HLS-Origin-Repräsentation normalisiert wird.

### `apply_outbound_content_coding_policy`

```rust
pub(crate) fn apply_outbound_content_coding_policy(
    headers: &mut HeaderMap,
    policy: OutboundContentCodingPolicy,
) {
    if matches!(policy, OutboundContentCodingPolicy::Identity) {
        force_accept_encoding_identity(headers);
    }
}
```

**Einsatz:** im finalen Request-Builder nach allen Merges sowie unmittelbar vor direkten HLS-Requests, die den generischen Builder umgehen.

### `parse_content_codings`

```rust
pub(crate) fn parse_content_codings(
    headers: &HeaderMap,
) -> Result<Vec<ContentCoding>, ContentCodingError>;
```

**Verhalten:**

- alle `Content-Encoding`-Headerwerte auswerten,
- kommagetrennte Tokens verarbeiten,
- Groß-/Kleinschreibung ignorieren,
- `identity` als No-op behandeln,
- `x-gzip` auf `Gzip` abbilden,
- Reihenfolge erhalten,
- unbekannte und leere/ungültige Tokens ablehnen.

Beispiel:

```http
Content-Encoding: gzip, br
```

ergibt:

```rust
vec![ContentCoding::Gzip, ContentCoding::Brotli]
```

Die Decoderkette wird in umgekehrter Reihenfolge aufgebaut:

```text
Brotli → gzip → Identity
```

### `decode_response_to_identity`

```rust
pub(crate) async fn decode_response_to_identity(
    response: reqwest::Response,
    detection: ContentCodingDetection,
) -> Result<DecodedHttpResponse, ContentCodingError>;
```

**Aufgaben:**

1. Status, finale URL und eine Headerkopie sichern.
2. `Content-Encoding` parsen.
3. Nur bei fehlendem Header und Manifest-Detection bekannte Signaturen prüfen.
4. `206` mit nichtleerer Decoderkette ablehnen.
5. `response.bytes_stream()` einmalig in den vorhandenen `DynReader` überführen.
6. Decoder in umgekehrter Reihenfolge streamend schachteln.
7. Decoder-I/O-Fehler mit `ContentDecodingIoError` kennzeichnen.
8. Nach tatsächlicher Transformation repräsentationsabhängige Header normalisieren.
9. `Content-Encoding: identity` als No-op normalisieren und aus der internen Headerkopie entfernen.

Nicht enthalten sind:

- eine zweite Range-Policy,
- generische Wire-/Decoded-Limits,
- Retry-Entscheidungen,
- Client-Header-Weitergabe,
- HLS-spezifische Cachelogik.

### `read_to_end_limited`

```rust
pub(crate) async fn read_to_end_limited<R>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Vec<u8>, ContentBodyReadError>
where
    R: AsyncRead + Unpin;
```

Der Helper liest höchstens `max_bytes + 1` dekodierte Bytes und meldet eine klare Limitüberschreitung.

### `read_utf8_limited`

```rust
pub(crate) async fn read_utf8_limited<R>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<String, ContentBodyReadError>
where
    R: AsyncRead + Unpin;
```

Der Helper delegiert an `read_to_end_limited` und führt anschließend eine strikte UTF-8-Prüfung aus. `String::from_utf8_lossy` ist unzulässig, weil
Replacement Characters Segment-URLs, Tokens oder Signaturen verändern können.

### `normalize_headers_after_content_decoding`

```rust
pub(crate) fn normalize_headers_after_content_decoding(
    headers: &mut HeaderMap,
    status: StatusCode,
);
```

Dieser private oder `pub(crate)` Helper wird ausschließlich vom Decoder aufgerufen und entfernt nach tatsächlicher Transformation:

```text
Content-Encoding
Content-Length
Content-Range
Accept-Ranges
ETag
Content-MD5
Digest
Content-Digest
Repr-Digest
Transfer-Encoding
```

Zusätzlich wird nur `Accept-Encoding` aus `Vary` entfernt. Andere `Vary`-Tokens bleiben erhalten.

Da ein codiertes `206` zuvor abgelehnt wird, normalisiert dieser Helper praktisch transformierte Vollantworten. Eine unveränderte Identity-`206` behält
ihre Range-Header.

### `content_decoding_error_from_io`

```rust
pub(crate) fn content_decoding_error_from_io(
    error: &io::Error,
) -> Option<&ContentDecodingIoError>;
```

**Einsatz:** in Shared-Cache-/Repair-Commitpfaden vor einer pauschalen Umwandlung in `CacheCommitError` oder Storage-Fehler.

## 8.2 HLS-spezifische Helper

### `force_identity_without_range`

Der vorhandene HLS-Helper bleibt bestehen und delegiert an den globalen Identity-Helper:

```rust
pub(crate) fn force_identity_without_range(headers: &mut HeaderMap) {
    headers.remove(header::RANGE);
    force_accept_encoding_identity(headers);
}
```

**Einsatz:**

- `build_hls_manifest_request_headers`,
- Shared-HLS-Resource-Header-Building,
- direkte HLS-Request-Pfade, bevor ein kontrollierter Range neu eingesetzt wird.

Er dokumentiert die HLS-Invariante früh. Das finale Enforcement im Request-Builder bleibt zusätzlich verpflichtend.

### DRY-Konsolidierung der Shared-Resource-Header

Die bestehenden Helper:

```rust
build_hls_origin_resource_headers
build_hls_origin_resource_headers_with_client_range
```

sollen auf einen internen Builder delegieren, damit Scrubbing, Provider-Session-Cookie, Identity und Range-Reihenfolge nur einmal implementiert sind:

```rust
enum HlsOriginRangeHeader {
    None,
    Parsed(ParsedByteRange),
    Forwarded(HeaderValue),
}

fn build_hls_origin_resource_headers_internal(
    source_headers: &HeaderMap,
    provider_session_headers: &HeaderMap,
    range: HlsOriginRangeHeader,
) -> Result<HeaderMap, HlsOriginResourceFetchError>;
```

Reihenfolge:

1. Header klonen und scrubben.
2. Provider-Session-Cookie kontrolliert anhängen.
3. `force_identity_without_range` anwenden.
4. ausschließlich den validierten/kontrollierten Range einsetzen.

### `mark_response_as_uncompressed`

Der bestehende Helper wird unverändert weiterverwendet.

**Verpflichtend für:**

- Legacy-Segment/Map/Key/Part/Other,
- Shared-Cache-Responses,
- Shared-Key-/Transient-Passthrough,
- sonstige binäre HLS-Responses.

**Nicht verwenden für:** neu erzeugte Manifest-Responses.

## 8.3 Bestehenden Textdecoder nicht duplizieren

`build_decoded_stream_reader` darf keine zweite Decoderimplementierung bleiben. Er wird entfernt oder delegiert an:

```rust
decode_response_to_identity(response, ContentCodingDetection::DeclaredOnly)
```

Bestehende generische Textdownloads erhalten dadurch vollständige Unterstützung für deklarierte Codierungen, aber **keine** neue globale
Magic-Sniffing-Semantik.

Der HLS-Manifestpfad wählt explizit:

```rust
ContentCodingDetection::DeclaredOrKnownHlsManifestMagic
```

---

## 9. Finales Outbound-Enforcement

### 9.1 `RequestFetchOptions` erweitern

```rust
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RequestFetchOptions {
    attempt_idle_timeout: Option<Duration>,
    content_coding: OutboundContentCodingPolicy,
}
```

Komfortkonstruktoren sollten bestehende Werte erhalten und benannte Varianten vermeiden, die einen positionalen `bool` einführen:

```rust
impl RequestFetchOptions {
    pub(crate) fn with_attempt_idle_timeout(timeout: Duration) -> Self {
        Self {
            attempt_idle_timeout: Some(timeout.max(Duration::from_millis(1))),
            ..Self::default()
        }
    }

    pub(crate) const fn with_content_coding(
        mut self,
        content_coding: OutboundContentCodingPolicy,
    ) -> Self {
        self.content_coding = content_coding;
        self
    }
}
```

### 9.2 Finalen `HeaderMap` im Request-Builder normalisieren

`get_client_request` baut derzeit intern aus konfigurierten und benutzerdefinierten Headern einen finalen `HeaderMap`. Genau dort muss die Policy nach
dem Merge angewandt werden.

Um bestehende Aufrufer nicht unnötig zu ändern, wird ein schmaler interner Builder empfohlen:

```rust
fn get_client_request_with_content_coding_policy<S>(
    client: &reqwest::Client,
    method: InputFetchMethod,
    headers: Option<&HashMap<String, String, S>>,
    url: &Url,
    custom_headers: Option<&HashMap<String, Vec<u8>, S>>,
    disabled_headers: Option<&ReverseProxyDisabledHeaderConfig>,
    default_user_agent: Option<&str>,
    content_coding: OutboundContentCodingPolicy,
) -> reqwest::RequestBuilder
where
    S: BuildHasher + Default;
```

Der bestehende `get_client_request` delegiert mit `Inherit`. Die Retry-/Provider-Funktionen mit `RequestFetchOptions` verwenden den neuen Builder und
reichen `options.content_coding` durch.

Dadurch gilt die Policy:

- nach `prepare_input_request_headers`,
- nach Input-/Client-Merge,
- bei jedem Retry,
- bei jedem Provider-URL-Versuch,
- bei jedem manuellen Redirect.

### 9.3 Manuelle Redirects

Im manuellen Redirectpfad wird `current_headers` nach Cross-Origin-Wechseln gescrubbt. Der Request-Builder wendet anschließend erneut die
Outbound-Policy an. Credentials bleiben entfernt; `Accept-Encoding: identity` bleibt beziehungsweise wird wiederhergestellt.

### 9.4 Shared-Manifest initial und Recovery

Initialer Shared-Manifest-Fetch:

```rust
RequestFetchOptions::with_attempt_idle_timeout(timeout)
    .with_content_coding(OutboundContentCodingPolicy::Identity)
```

Recovery-Direct-Targets umgehen den generischen Request-Builder. In `fetch_origin_manifest_once` beziehungsweise unmittelbar vor `.send()` wird ein
Header-Clone abschließend mit:

```rust
apply_outbound_content_coding_policy(
    &mut headers,
    OutboundContentCodingPolicy::Identity,
);
```

normalisiert.

### 9.5 Legacy-Manifest

`build_hls_manifest_request_headers` verwendet weiterhin `force_identity_without_range`, damit der Headerzustand bereits in der HLS-Orchestrierung
korrekt ist. Der tatsächliche Textdownload setzt zusätzlich `RequestFetchOptions.content_coding = Identity`; dies ist das verbindliche Enforcement
nach dem späteren Input-Merge.

### 9.6 Legacy-Provider-Ressourcen

Der Legacy-Provider-Stream erhält einen expliziten, schmalen Repräsentationsmodus; dieser setzt `Accept-Encoding: identity` beim finalen Bau jedes
Provider-Requests. Eine Ableitung ausschließlich aus `PlaylistItemType::LiveHls` ist unzulässig, weil HLS-Catchup als `Catchup` klassifiziert sein
kann.

---

## 10. Integration der Response-Dekompression

### 10.1 Beide Manifestpfade

Für Manifeste gilt:

```rust
let decoded_body = timeout(
    Duration::from_millis(origin_manifest_timeout_ms.max(1)),
    async {
        let mut decoded = decode_response_to_identity(
            response,
            ContentCodingDetection::DeclaredOrKnownHlsManifestMagic,
        )
        .await?;

        let body = read_utf8_limited(
            &mut decoded.body,
            MAX_HLS_MANIFEST_BYTES,
        )
        .await?;

        Ok::<_, ManifestBodyError>((decoded, body))
    },
)
.await
.map_err(|_| OriginManifestFetchError::Timeout)??;
```

Die konkrete Fehlerhülle kann dem bestehenden Modul angepasst werden; verbindlich ist, dass Decoder-Vorbereitung, Prefix-Read, vollständiges Lesen,
Limit und UTF-8-Prüfung unter derselben Deadline liegen.

#### Shared-Manifest

`read_origin_manifest_body` liest nicht mehr direkt `response.bytes_stream()` in einen `Vec<u8>`. `response_to_fetched_manifest` beziehungsweise ein
naher Helper:

1. extrahiert Status und finale URL,
2. dekodiert unter Manifest-Deadline,
3. übernimmt weiterhin Provider-Session-Cookies aus der internen Headerkopie,
4. mappt Setup-, Limit-, UTF-8- und streamende Decoderfehler strukturiert auf `OriginManifestFetchError`.

`MAX_HLS_MANIFEST_BYTES` gilt auf die **dekodierte** UTF-8-Repräsentation. Ein vorhandenes `Content-Length` kann als schneller, konservativer
Preflight verwendet werden, ist aber nicht das autoritative Decoded-Limit.

#### Legacy-Manifest

Der bestehende Textdownload-Stack delegiert an den zentralen Decoder. Generische Textaufrufer verwenden `DeclaredOnly`; nur der HLS-Manifestaufrufer
aktiviert `DeclaredOrKnownHlsManifestMagic` und ein HLS-spezifisches Decoded-Limit.

Das umgeschriebene Manifest wird ohne `mark_response_as_uncompressed` erzeugt, damit die vorhandene Tower-Schicht eine Client-Kompression aushandeln
kann.

### 10.2 Shared-Segment, Map und cachefähige Transient-Ressourcen

Der Shared-HLS-Resource-Retry-Loop ist der zentrale Integrationspunkt. Es wird **keine** `response_policy` pro
`HlsOriginResourceFetchTarget` eingeführt, weil alle Shared-HLS-Ressourcen dieselbe Identity-Invariante besitzen.

Reihenfolge:

```text
Origin-Response
    ↓
vorhandene Status-/HlsOriginByteRangeExpectation-Prüfung
    ↓
decode_response_to_identity(DeclaredOnly)
    ↓
Commit-Closure erhält DecodedHttpResponse
```

Die Commit-Closure ändert sich konzeptionell von:

```rust
FnMut(reqwest::Response, HlsResourceFetchAttempt) -> ...
```

zu:

```rust
FnMut(DecodedHttpResponse, HlsResourceFetchAttempt) -> ...
```

Segment-, Map- und Transient-Cachewriter verwenden ausschließlich:

```rust
decoded.body
```

Der bestehende Cache-Writer begrenzt über `max_object_bytes`. Weil der Decoder davor sitzt, gilt das vorhandene Limit automatisch für die
Identity-Repräsentation. Ein zweites generisches Decoded-Limit im Decoder ist nicht erforderlich.

Vor der Umwandlung von Body-I/O-Fehlern in `CacheCommitError` wird mit `content_decoding_error_from_io` geprüft, ob ein Content-Decoderfehler vorliegt.
Decoderfehler werden als Origin-/Content-Fehler klassifiziert und nicht als Storage-, Disk-Full- oder Cache-I/O-Fehler.

### 10.3 Segment Repair

Segment Repair erhält die dekodierte Identity-Repräsentation. Die HLS-Inhaltsverschlüsselung bleibt unberührt:

- HTTP `Content-Encoding` wird entfernt,
- AES-/HLS-verschlüsselte Segmentbytes bleiben exakt erhalten,
- ffmpeg sieht denselben Medien-/Ciphertext-Body, den ein standardskonformer Origin bei `identity` liefern würde.

### 10.4 Shared-Key und Transient-Passthrough

`hls_transient_origin_response` erhält statt einer rohen `reqwest::Response` eine `DecodedHttpResponse`.

Die Client-Response:

- verwendet den dekodierten `body`,
- wählt nur sichere Response-Header aus `decoded.headers`,
- besitzt nach Transformation kein `Content-Encoding` und keine veraltete `Content-Length`,
- wird mit `mark_response_as_uncompressed` markiert.

Ein Decoderfehler während des Streamings beendet den Client-Stream. Ein transparenter Retry nach gesendeten Response-Headern ist nicht möglich und
wird weder versprochen noch simuliert.

### 10.5 Legacy-Provider-Stream

Für Legacy wird ein schmaler fachlicher Modus eingeführt:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ProviderContentRepresentationMode {
    #[default]
    PreserveOrigin,
    HlsIdentity,
}
```

Dieser Modus steuert zusammenhängend:

- ob der finale Origin-Request `Accept-Encoding: identity` erzwingt,
- ob die Provider-Response nach Identity dekodiert wird,
- wie Response-Header ausgewählt werden.

Der Modus wird explizit durch alle relevanten Strukturen und Pfade weitergereicht:

```text
ForceStreamRequestContext
    → create_stream_response_details
    → ProviderStreamFactoryParams
    → ProviderStreamFactoryOptions
    → initialer Provider-Open
    → deferred Provider-Open
    → Reconnect / Provider-Wechsel
    → provider_stream_request
```

HLS-Live und HLS-Catchup setzen `HlsIdentity`. Andere Provider-Streams bleiben `PreserveOrigin`.

Im `HlsIdentity`-Modus:

```rust
let decoded = decode_response_to_identity(
    response,
    ContentCodingDetection::DeclaredOnly,
)
.await?;
```

Der resultierende `DynReader` wird streamend in den bestehenden Provider-Stream überführt. Streamzeitige Decoderfehler beenden den Client-Stream und
werden als Content-Decoding-Fehler geloggt.

### 10.6 Nicht-HLS-`PreserveOrigin`

Im Preserve-Modus wird der Decoder nicht aufgerufen:

- Body bleibt bytegenau unverändert,
- `Content-Encoding` muss gemeinsam mit dem Body erhalten bleiben,
- `Content-Length` und `Content-Range` bleiben repräsentationskonsistent,
- `Transfer-Encoding` wird beim Neubau der Axum-Response nicht übernommen,
- bestehende Schutzmechanismen gegen Tower-Doppelkompression bleiben erhalten.

Diese Logik behebt den bestehenden Grundfehler „codierter Body ohne `Content-Encoding`“, ohne globale Headerfreigaben einzuführen.

---

## 11. Sichere Response-Header-Auswahl

Die bestehende Allowlist-Strategie bleibt grundsätzlich bestehen. Es wird kein `copy_end_to_end_response_headers` eingeführt.

Für Provider-Streams entsteht ein enger Helper:

```rust
pub(crate) fn provider_response_headers(
    origin_headers: &HeaderMap,
    mode: ProviderContentRepresentationMode,
) -> HeaderMap;
```

### 11.1 Preserve-Modus

- bestehende sichere Header übernehmen,
- `Content-Encoding` explizit zusätzlich übernehmen,
- `Content-Length` und `Content-Range` erhalten,
- `Transfer-Encoding` niemals übernehmen.

### 11.2 HLS-Identity-Modus

- bestehende sichere Header aus der bereits normalisierten `DecodedHttpResponse.headers` übernehmen,
- kein `Content-Encoding`,
- keine veraltete `Content-Length`,
- keine codierungsbezogenen Validatoren,
- `Content-Type`, Cache-Header und andere explizit erlaubte Header behalten.

### 11.3 Globale Allowlist

`SUPPORTED_RESPONSE_HEADERS` wird nicht pauschal durch eine permissive End-to-End-Kopie ersetzt.

`Transfer-Encoding` muss nach Audit aller Aufrufer aus Pfaden entfernt werden, die eine neue Axum-/Hyper-Response erzeugen. `Content-Encoding` wird
nicht blind global freigeschaltet, sondern im Provider-`PreserveOrigin`-Helper gezielt behandelt, sofern ein Aufruferkontext andernfalls nicht sicher
unterscheidbar ist.

---

## 12. Deflate-Behandlung

Der bestehende Helper:

```rust
is_deflate(...)
```

wird präzisiert zu:

```rust
pub const fn is_zlib_header(bytes: &[u8]) -> bool {
    if bytes.len() < 2 {
        return false;
    }

    let cmf = bytes[0];
    let flg = bytes[1];
    let header = u16::from(cmf) << 8 | u16::from(flg);

    let compression_method_is_deflate = cmf & 0x0f == 8;
    let window_size_is_valid = cmf >> 4 <= 7;
    let checksum_is_valid = header % 31 == 0;

    compression_method_is_deflate
        && window_size_is_valid
        && checksum_is_valid
}
```

Bei:

```http
Content-Encoding: deflate
```

wird ein kurzer Prefix unter der bestehenden Deadline betrachtet:

- gültiger zlib-Header → `ZlibDecoder`,
- sonst → `DeflateDecoder` für Raw-Deflate.

Der bisherige Name kann für einen kleinen Übergang als delegierender Kompatibilitätswrapper bestehen bleiben, sofern andere Aufrufer im selben Change
nicht sicher migriert werden können:

```rust
pub const fn is_deflate(bytes: &[u8]) -> bool {
    is_zlib_header(bytes)
}
```

---

## 13. Dependency-Anpassung

`async-compression` wird erweitert:

```toml
async-compression = {
    version = "0.4.42",
    features = [
        "tokio",
        "gzip",
        "zlib",
        "deflate",
        "brotli",
        "zstd"
    ]
}
```

`Cargo.toml` und `Cargo.lock` werden gemeinsam aktualisiert.

Automatische reqwest-Kompressionsfeatures werden nicht aktiviert. Eine globale Autodekompression würde:

- selbst `Accept-Encoding` beeinflussen,
- Nicht-HLS-Pfade verändern,
- Range-Semantik implizit beeinflussen,
- Origin-Header vor der Tuliprox-Policy verändern,
- Fehler- und Deadline-Klassifikation erschweren.

Die explizite, streamende Tuliprox-Dekompression bleibt daher die kontrollierte Quelle der Wahrheit.

---

## 14. Fehlerklassifikation und Retry-Disposition

### 14.1 Manifest

`OriginManifestFetchError` erhält klar unterscheidbare Varianten beziehungsweise Mappings für:

- ungültigen/unsupported `Content-Encoding`,
- codiertes `206`,
- Content-Decoder-I/O-Fehler,
- Decoded-Limitüberschreitung,
- ungültiges UTF-8,
- Timeout.

Ein Decoderfehler darf nicht mehr als generischer UTF-8-Fehler erscheinen.

### 14.2 Shared-Resource

`HlsOriginResourceFetchError` erhält eine Content-Coding-Variante oder eine äquivalente strukturierte HLS-Fehlerrepräsentation. Die konkrete clonbare
Fehlerform kann HLS-spezifisch sein; die globale Decoder-API bleibt davon unabhängig.

Die Retry-Entscheidung wird an einer Stelle gebündelt, beispielsweise:

```rust
fn classify_hls_resource_failure(
    error: &HlsOriginResourceFetchError,
) -> HlsResourceRetryDisposition;
```

### 14.3 Disposition

| Fehler | Buffered/Cachewrite | Direkter Stream nach Response-Beginn |
| --- | --- | --- |
| abgebrochener/korrupter gzip/br/zstd/deflate-Body | retryfähig gemäß bestehender Policy | Stream abbrechen |
| temporärer Body-I/O-Fehler | retryfähig gemäß bestehender Policy | Stream abbrechen |
| unbekanntes `Content-Encoding` | nicht auf demselben Target retryfähig | Response vor Beginn ablehnen |
| codiertes `206` | nicht retryfähig als normaler Contentfehler | Response vor Beginn ablehnen |
| Decoded-Limit überschritten | nicht retryfähig | nicht zutreffend oder Streamlimit des Consumers |
| ungültiges UTF-8 nach erfolgreicher Dekompression | nicht retryfähig | nur Manifestpfad |
| Timeout | bestehende Timeout-/Retry-Policy | Stream abbrechen, wenn bereits begonnen |

---

## 15. Größenlimits und Ressourcenverbrauch

### 15.1 Manifest

- Autoritatives Limit: dekodierte UTF-8-Repräsentation, aktuell `MAX_HLS_MANIFEST_BYTES`.
- `read_utf8_limited` liest höchstens `limit + 1` Bytes.
- `Content-Length` darf nur als schneller konservativer Preflight dienen.
- Decoder-Vorbereitung und Lesen liegen unter derselben Manifest-Deadline.

### 15.2 Shared-Cache-Objekte

- Das vorhandene `max_object_bytes` bleibt maßgeblich.
- Der Decoder sitzt vor `reader.take(max_object_bytes + 1)` beziehungsweise dem bestehenden Cachewriter-Limit.
- Das Limit gilt dadurch automatisch für die gespeicherte Identity-Repräsentation.

### 15.3 Direkte Streams

- Keine Vollpufferung beliebig großer Segmente.
- Keine neue generische Body-Größenpolicy im Decoder.
- Bestehende Idle-/Body-/Client-Send-Deadlines bleiben maßgeblich.
- Dekompression erfolgt mit Backpressure über `AsyncRead`/`ReaderStream`.

---

## 16. Logging und begrenzte Observability

Bei abweichend codierten HLS-Origin-Antworten werden sichere strukturierte Logs erzeugt:

```text
object_kind=manifest|segment|map|key|part|other
content_encoding=gzip|deflate|br|zstd|multiple
status=200|206
requested_accept_encoding=identity
decoded_to_identity=true|false
content_length=<n|unknown>
range_requested=true|false
source=legacy|shared
```

Nicht loggen:

- vollständige signierte URLs,
- Query-Tokens,
- Cookies,
- Key-Bodies,
- Manifestinhalte.

Die aktuelle Metrikarchitektur verwendet feste `AtomicU64`-Felder und keine Labels. Für diesen Change wird keine neue Label-Infrastruktur eingeführt.
Optional in einer späten Phase:

```rust
origin_encoded_responses: AtomicU64,
origin_decode_failures: AtomicU64,
origin_encoded_partial_rejections: AtomicU64,
```

Eine gemeinsame gelabelte Legacy-/Shared-Metrikarchitektur ist ein separates Observability-Vorhaben.

---

## 17. Testkonzept

## 17.1 Globaler Content-Coding-Layer

- Identity ohne Header.
- explizites `Content-Encoding: identity` wird als No-op normalisiert.
- gzip per Header.
- `x-gzip`.
- zlib-wrapped Deflate.
- Raw-Deflate.
- Brotli.
- Zstandard.
- `gzip, br` wird in umgekehrter Reihenfolge dekodiert.
- mehrere `Content-Encoding`-Header.
- Groß-/Kleinschreibung.
- ungültiger leerer Token.
- unbekannte Codierung.
- abgeschnittener gzip-/Brotli-/Zstd-Body erzeugt `ContentDecodingIoError`.
- `206` plus gzip wird abgelehnt.
- `206` plus Identity bleibt unverändert.
- Headernormalisierung nach Transformation.
- `Vary: Accept-Encoding, Origin` wird zu `Vary: Origin`.
- `Transfer-Encoding` wird nicht in Client-Header übernommen.

## 17.2 Detection-Priorität

- deklarierte Codierung gewinnt gegen widersprechende Magic-Bytes.
- `Content-Encoding: br` plus gzip-Magic führt zu Brotli-Fehler, nicht zu gzip-Fallback.
- gzip-Magic ohne Header wird nur bei `DeclaredOrKnownHlsManifestMagic` erkannt.
- generischer Textdownload mit `DeclaredOnly` rät keine Codierung.
- binärer Key ohne Header, der mit `1f 8b` beginnt, bleibt unverändert.

## 17.3 Finales Request-Enforcement

- Client sendet `Accept-Encoding: br`; finaler HLS-Origin-Request enthält `identity`.
- Input konfiguriert `Accept-Encoding: gzip`; finaler HLS-Origin-Request enthält `identity`.
- Input und Client setzen unterschiedliche Werte; `identity` gewinnt.
- Policy bleibt bei Retry erhalten.
- Policy bleibt bei Provider-URL-Wechsel erhalten.
- Policy bleibt bei Same-Origin-Redirect erhalten.
- Policy wird nach Cross-Origin-Credential-Scrubbing erneut angewandt.
- direkte Shared-HLS-Recovery-Requests enthalten `identity`.

## 17.4 Legacy-Manifest

- Provider respektiert `identity`; Manifest wird korrekt umgeschrieben.
- Provider ignoriert `identity` und liefert gzip-Manifest.
- Brotli-Manifest.
- Zstandard-Manifest.
- Raw-Deflate-Manifest.
- Magic-gzip ohne Header nur im HLS-Pfad.
- Decoded-Limit gilt nach Dekompression.
- Timeout umfasst Prefix-Erkennung und Decoder-Vorbereitung.
- neu erzeugtes Manifest ist nicht mit `mark_response_as_uncompressed` markiert und bleibt Tower-komprimierbar.

## 17.5 Shared-Manifest

- komprimiertes Manifest wird dekodiert und committed.
- Provider-Session-Cookies bleiben intern auswertbar.
- `MAX_HLS_MANIFEST_BYTES` gilt nach Dekompression.
- ungültiges UTF-8 wird von Decoderfehlern unterschieden.
- codiertes `206` wird abgelehnt.
- Recovery-Pfad verwendet dieselbe Content-Coding-Logik.

## 17.6 Shared-Cache-Ressourcen

- gzip-/br-/zstd-Segment wird vor Repair und Cache dekodiert.
- Cachedatei beginnt mit den erwarteten Medienbytes, nicht mit einer Kompressionshülle.
- fMP4-Map wird als Identity gecacht.
- Range aus dem Shared-Cache bezieht sich auf die dekodierte Datei.
- Decoderfehler entfernt temporäre Cachedateien.
- Decoderfehler wird nicht als Disk-Full/Storage-/Cache-I/O klassifiziert.
- vorhandene `HlsOriginByteRangeExpectation` bleibt vor dem Decoder maßgeblich.
- codiertes `206` wird nicht versehentlich als gültiger Partial-Cachewrite akzeptiert.

## 17.7 Shared-Transient und Key

- komprimierter Key wird als exakte dekodierte Bytefolge ausgeliefert.
- komprimierte Map/Part/Other-Passthrough-Response wird dekodiert.
- keine Magic-Erkennung bei Binärressourcen.
- Response besitzt nach Transformation kein `Content-Encoding` und keine veraltete `Content-Length`.
- Response ist mit `mark_response_as_uncompressed` markiert.
- Decoderfehler nach Response-Beginn beendet den Stream; Test erwartet keinen transparenten Retry.

## 17.8 Legacy-Provider-Streams

- gzip-Segment wird als dekodiertes Segment ohne `Content-Encoding` ausgeliefert.
- gzip-Key wird als dekodierte Key-Bytes ausgeliefert.
- gzip-Map wird als dekodierte Init-Map ausgeliefert.
- codiertes `206` wird abgelehnt.
- HLS-Catchup setzt explizit `HlsIdentity`.
- initialer Provider-Open behält den Modus.
- deferred Provider-Open behält den Modus.
- Reconnect/Provider-Wechsel behält den Modus.
- direkte Streamdecoderfehler führen zu Streamabbruch, nicht zu einem fingierten Retry.
- binäre HLS-Response ist gegen Tower-Doppelkompression markiert.

## 17.9 Nicht-HLS-Preserve-Modus

- Body bleibt bytegenau erhalten.
- `Content-Encoding` wird zusammen mit dem unveränderten Body weitergegeben.
- `Content-Length` und `Content-Range` bleiben konsistent.
- `Transfer-Encoding` wird nicht in die neu erzeugte Client-Response kopiert.
- keine unbeabsichtigte Dekompression oder neue Magic-Erkennung.

---

## 18. Implementierungsphasen gemäß `AGENTS.md`

Jede Phase wird als eigener kleiner, reviewbarer und kompilierbarer Agent-/PR-Scope behandelt. Vor Änderungen liest der Agent:

1. die Root-`AGENTS.md`,
2. jede weitere anwendbare `AGENTS.md` im betroffenen Verzeichnisbaum,
3. die bindende HLS-Zielspezifikation:

```text
dev/concepts/hls-object-cache/xtream_hls_cache_proxy_zielkonzept_v19.md
```

Fehlt diese Datei im Arbeitsbaum, darf ihr Inhalt nicht geraten werden. Der Agent dokumentiert den Blocker beziehungsweise arbeitet nur an einem
Scope, der nachweislich ohne Annahmen über die fehlende Spezifikation möglich ist.

### Phase 1: Decoder-Fundament und beide Manifestpfade

Scope:

1. `async-compression`-Features und Lockfile.
2. schlankes `content_coding`-Modul.
3. finale `OutboundContentCodingPolicy` im generischen Request-Builder.
4. direkte Shared-Manifest-Recovery-Requests absichern.
5. Legacy- und Shared-Manifest auf zentralen Decoder umstellen.
6. Manifest-, Header-Merge-, Redirect-, Deadline-, UTF-8- und Limit-Tests.

Bewusst nicht enthalten:

- Shared-Segment-/Map-Cache,
- Legacy-Binärstreams,
- Transient-Passthrough,
- neue Metrikarchitektur.

### Phase 2: Shared-HLS gecachte Ressourcen

Scope:

1. Shared-Resource-Retry-Loop zentral auf `DecodedHttpResponse` umstellen.
2. Segment, Map und cachefähige Transient-Ressourcen.
3. Decoderfehler vor Cache-/Storage-Mapping erkennen.
4. Repair-, Cache-, Range- und Cleanup-Tests.

Bewusst nicht enthalten:

- Legacy-Provider-Streams,
- direkte Key-/Transient-Clientstreams,
- breite Header-Refactorings.

### Phase 3: Direkte HLS-Streams

Scope:

1. Shared-Key-/Transient-Passthrough.
2. `ProviderContentRepresentationMode` für Legacy.
3. Legacy-Segment/Map/Key/Part/Other einschließlich Catchup.
4. sichere Provider-Response-Header-Auswahl.
5. `mark_response_as_uncompressed` an allen Binärpfaden.
6. initial/deferred/reconnect-Modus-Propagation und Streaming-Fehler-Tests.

Bewusst nicht enthalten:

- neue gelabelte Metrikarchitektur,
- allgemeiner Proxy-Framework-Umbau.

### Phase 4: Cleanup und begrenzte Observability

Scope:

1. alte Decoderduplikate entfernen oder delegieren lassen.
2. `is_zlib_header` und kontrollierte Migration.
3. `Transfer-Encoding`-Weitergabe nach Callsite-Audit beseitigen.
4. sichere Logs und optional feste Counter.
5. vollständige Regressionstests und Dokumentationsabgleich.

### Erforderliches Agent-Ergebnis je Phase

Gemäß `AGENTS.md` endet jede Phase mit:

```text
Changed files:
New files:
Acceptance criteria satisfied:
Tests / validation performed:
Open points / blockers:
```

Kein Agent-Auftrag darf die Formulierung „implementiere das vollständige HLS-Konzept“ als Erlaubnis für angrenzende Phasen verwenden.

---

## 19. Nicht-Ziele

Nicht Bestandteil dieses Konzepts sind:

- Cachevarianten pro `Content-Encoding`,
- automatische reqwest-Dekompression,
- eine neue generische Proxy-Headerplattform,
- pauschales Kopieren aller End-to-End-Header,
- eine zweite Range-Policy neben `HlsOriginByteRangeExpectation`,
- Vollpufferung beliebig großer Segmente,
- eine neue gelabelte Metrikarchitektur,
- Änderungen an HLS-Inhaltsverschlüsselung oder Key-Material,
- stillschweigende Reparatur unbekannter oder widersprüchlich deklarierter Codierungen.

---

## 20. Abschließende Zielmatrix

| HLS-Objekt | Finaler Origin-Request | Origin-Fallback | Interne Repräsentation | Client-Ausgabe | Retry-Grenze |
| --- | --- | --- | --- | --- | --- |
| Manifest | `Accept-Encoding: identity`, kein Client-Range | Header; nur ohne Header HLS-Text-Magic; gzip/deflate/br/zstd | strikter UTF-8-Identity-String | neu erzeugt; Tower darf komprimieren | vor Ausgabe vollständig retryfähig |
| Segment | `identity`, kontrollierter Range | nur deklarierte Codierung | Identity-Medien-/Ciphertextbytes | Identity, keine Tower-Kompression | Cachewrite retryfähig; Direct-Stream nach Beginn nicht |
| Map | `identity`, kontrollierter Range | nur deklarierte Codierung | Identity-Init-Bytes | Identity, keine Tower-Kompression | Cachewrite retryfähig; Direct-Stream nach Beginn nicht |
| Key | `identity`, kontrollierter Range | nur deklarierte Codierung | Identity-Key-Bytes | Identity, keine Tower-Kompression | Direct-Stream nach Beginn nicht |
| Part/Other | `identity`, kontrollierter Range | nur deklarierte Codierung | Identity-Bytes | Identity, keine Tower-Kompression | abhängig von Cachewrite oder Direct-Stream |
| Nicht-HLS Preserve | bestehende Negotiation | keine Tuliprox-Dekompression | Origin-Repräsentation | Body und `Content-Encoding` gemeinsam | bestehendes Streamverhalten |

Die zentralen Invarianten lauten:

1. `Accept-Encoding: identity` wird am **finalen** HLS-Origin-Request-Boundary erzwungen.
2. Ein Provider, der `identity` ignoriert, wird über einen gemeinsamen streamenden Decoder korrekt verarbeitet.
3. Parser, Repair und Shared Cache sehen ausschließlich Identity-Bytes.
4. Ein codiertes `206 Partial Content` wird nicht dekodiert, sondern abgelehnt.
5. HLS-Manifest-Magic-Sniffing ist eng begrenzt und überschreibt nie einen deklarierten Header.
6. Binäre HLS-Responses werden nicht durch Tower erneut komprimiert.
7. Nicht-HLS-Preserve-Responses behalten Body und `Content-Encoding` konsistent gemeinsam.
8. Decoder-, Cache-, Range-, Retry- und Header-Zuständigkeiten bleiben getrennt und werden nicht doppelt modelliert.
