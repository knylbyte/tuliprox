# Coding-Agent-Auftrag: Phase 1 – Decoder-Fundament und Manifestpfade

Implementiere ausschließlich **Phase 1** der Tuliprox-Anpassung für HTTP-`Content-Encoding` bei HLS.

Die bindende technische Spezifikation für diesen Auftrag ist:

```text
dev/concepts/hls-content-encoding/hls_content_encoding_policy_and_processing_concept.md
```

Maßgeblich sind insbesondere die dort beschriebenen Zielinvarianten, der schlanke globale Content-Coding-Layer, das
finale Outbound-Enforcement, die Integration beider Manifestpfade, die Fehler- und Deadline-Regeln, die
Phase-1-Testmatrix sowie **Phase 1** im Abschnitt „Implementierungsphasen gemäß `AGENTS.md`“.

## Verbindliche Vorbereitung

Führe vor jeder Codeänderung diese Schritte in der genannten Reihenfolge aus:

1. Lies die `AGENTS.md` im Repository-Root vollständig.
2. Suche nach weiteren `AGENTS.md` in und oberhalb aller Verzeichnisse, die du voraussichtlich bearbeitest, und befolge
   sämtliche anwendbaren Vorgaben.
3. Lies das Content-Encoding-Konzept vollständig.
4. Lies die in der aktuellen Root-`AGENTS.md` als bindend bezeichnete HLS-Zielspezifikation. Zum Zeitpunkt der
   Prompt-Erstellung lautet der dort genannte Pfad:

   ```text
   dev/concepts/hls-object-cache/xtream_hls_cache_proxy_zielkonzept_v19.md
   ```

   Nennt die aktuelle `AGENTS.md` einen anderen Pfad, ist ausschließlich dieser aktuelle Pfad maßgeblich.
5. Prüfe den aktuellen Branch auf bereits vorhandene oder teilweise implementierte Content-Coding-Änderungen. Verwende
   vorhandene korrekte Implementierungen nach DRY und erzeuge keine parallele Decoder- oder Header-Policy.

Fehlt eine laut `AGENTS.md` bindende Spezifikation, darfst du ihren Inhalt nicht erraten. Dokumentiere den Blocker vor
Änderungen. Arbeite nur weiter, wenn der Phase-1-Scope nachweislich ohne Annahmen über die fehlende Spezifikation
umgesetzt werden kann.

## Arbeitsmodus

Arbeite zuerst im Plan-Modus. Liefere vor der ersten Änderung einen konkreten Plan mit:

1. zu ändernden Dateien,
2. neu anzulegenden Dateien,
3. exaktem Scope dieser Phase,
4. Tests und lokalen Testserver-Fixtures,
5. Risiken und Annahmen,
6. ausdrücklich nicht umgesetzten Teilen des Content-Encoding-Konzepts und der allgemeinen HLS-Zielspezifikation.

Beginne erst danach mit der Implementierung. Halte jeden Zwischenschritt kompilierbar und reviewbar. Ändere keine
angrenzenden Phasen und führe keine spekulativen Framework-Refactorings durch.

## Scope dieser Phase

Setze ausschließlich die folgenden Arbeitspakete aus dem Konzept um:

1. Ergänze die benötigten `async-compression`-Features für gzip, zlib, Raw-Deflate, Brotli und Zstandard. Aktualisiere
   `Cargo.toml` und `Cargo.lock` gemeinsam.
2. Implementiere den im Konzept definierten schlanken, crate-internen Content-Coding-Layer. Verwende den vorhandenen
   `DynReader`; führe keine redundante Reader-, Range- oder generische Response-Policy-Hierarchie ein.
3. Unterstütze deklarierte Codierungen einschließlich `identity`, `gzip`, `x-gzip`, `deflate`, `br`, `zstd`, mehrerer
   `Content-Encoding`-Header und mehrstufiger Codierungen in korrekter umgekehrter Dekodierreihenfolge.
4. Implementiere die im Konzept festgelegte Detection-Priorität. Magic-Sniffing ist ausschließlich für HLS-Manifeste
   ohne `Content-Encoding` zulässig. Deklarierte Codierungen haben immer Vorrang. Binäre Ressourcen und generische
   Textdownloads dürfen keine neue Magic-Erkennung erhalten.
5. Implementiere die schlanke `OutboundContentCodingPolicy` im bestehenden Request-Stack und erzwinge `Accept-Encoding:
   identity` am finalen Request-Boundary **nach allen Header-Merges**.
6. Stelle sicher, dass die Identity-Policy bei Retry, Provider-URL-Wechsel, Same-Origin-Redirect, Cross-Origin-Redirect
   nach Credential-Scrubbing sowie bei direkten Shared-Manifest-Recovery-Requests erhalten bleibt.
7. Stelle Legacy- und Shared-HLS-Manifeste auf den zentralen Decoder um.
8. Sorge dafür, dass Decoder-Vorbereitung, Prefix-Read, vollständiger Bodykonsum, Decoded-Limit und UTF-8-Prüfung unter
   derselben bestehenden Manifest-Deadline liegen.
9. Wende das Manifestlimit auf die dekodierte UTF-8-Repräsentation an. Verwende `Content-Length` höchstens als
   konservativen Preflight, nicht als autoritatives Decoded-Limit.
10. Unterscheide strukturierte Content-Coding-, Limit-, Timeout- und UTF-8-Fehler. Reiche unbekannte oder
    widersprüchlich deklarierte Codierungen nicht stillschweigend als Text weiter.
11. Erhalte die interne Auswertung von Provider-Session-Cookies im Shared-Manifestpfad.
12. Lasse neu erzeugte Legacy- und Shared-Manifeste weiterhin durch die bestehende Tower-Kompressionsschicht für den
    Client aushandeln. Markiere Manifest-Responses nicht als unkomprimierbar.
13. Lass bestehende generische Textdownloads an die zentrale deklarationsbasierte Decoderlogik delegieren, sofern dies
    für DRY erforderlich ist. Ändere ihre Semantik nicht durch globales HLS-Magic-Sniffing.

Konkrete Typnamen, Verantwortlichkeiten und Zuständigkeitsgrenzen aus dem Konzept sind verbindlich. Signaturen dürfen an
den aktuellen Branch angepasst werden, sofern die Lösung mindestens gleich schmal bleibt und keine duplizierte
Implementierung erzeugt.

## Explizit nicht enthalten

Implementiere in dieser Phase nicht:

- Shared-Segment-, Map- oder Transient-Cachewrites,
- Segment Repair für komprimierte Origin-Antworten,
- Shared-Key- oder Transient-Passthrough,
- Legacy-Segment-, Map-, Key-, Part- oder andere binäre Provider-Streams,
- `ProviderContentRepresentationMode` beziehungsweise dessen spätere Entsprechung,
- allgemeine Provider-Response-Header-Refactorings,
- eine zweite Range-Policy neben bestehenden HLS-Typen,
- automatische reqwest-Dekompression,
- gelabelte Metriken oder einen allgemeinen Observability-Umbau,
- pauschales Kopieren aller End-to-End-Header.

Stößt du auf eine notwendige Änderung, die eindeutig zu Phase 2, 3 oder 4 gehört, dokumentiere sie unter „Open points /
blockers“ und implementiere sie nicht.

## Akzeptanzkriterien

Die Phase gilt nur dann als abgeschlossen, wenn mindestens Folgendes nachweisbar ist:

- Der zentrale Decoder verarbeitet alle im Konzept für Phase 1 geforderten deklarierte Codierungen korrekt und
  streamend.
- Raw-Deflate und zlib-wrapped Deflate werden korrekt unterschieden.
- Mehrere Codierungen werden in umgekehrter Anwendungsreihenfolge dekodiert.
- Ein deklarierter Header gewinnt gegen widersprechende Magic-Bytes.
- Magic-Sniffing ohne Header ist auf den HLS-Manifestmodus begrenzt.
- Ein binärer Body ohne Header wird nicht anhand zufälliger Magic-Bytes dekomprimiert.
- `Accept-Encoding: identity` gewinnt am finalen HLS-Origin-Request gegen Client- und Input-Header.
- Die Policy bleibt über Retry, Provider-URL-Wechsel und manuelle Redirects erhalten.
- Direkte Shared-Manifest-Recovery-Requests senden ebenfalls `identity`.
- Legacy- und Shared-Manifeste werden bei gzip, Raw-Deflate, Brotli und Zstandard korrekt dekodiert.
- Das Manifestlimit gilt nach Dekompression.
- Die bestehende Manifest-Deadline umfasst auch Decoder-Vorbereitung und Prefix-Erkennung.
- Ungültiges UTF-8 wird von einem Decoderfehler unterschieden.
- Shared Provider-Session-Cookies bleiben intern nutzbar.
- Neu erzeugte Manifeste bleiben Tower-komprimierbar.
- Es existiert keine zweite, parallele Decoderimplementierung für denselben Zweck.

## Tests

Implementiere die Phase-1-relevanten Tests aus dem Konzept. Dazu gehören mindestens:

- Unit-Tests des globalen Content-Coding-Layers,
- Detection-Priorität und widersprüchliche Header/Magic-Bytes,
- mehrere Header und mehrstufige Codierungen,
- abgeschnittene oder korrupte komprimierte Bodies,
- finales Request-Enforcement nach Input-/Client-Merge,
- Retry-, Provider-Wechsel- und Redirect-Erhalt der Policy,
- direkte Shared-Recovery-Requests,
- Legacy-Manifesttests für alle unterstützten Codierungen,
- Shared-Manifesttests für alle unterstützten Codierungen,
- Decoded-Limit, Deadline und UTF-8-Abgrenzung,
- Nachweis, dass Manifest-Responses nicht mit `mark_response_as_uncompressed` markiert werden.

Für Netzwerkverhalten sind ausschließlich lokale Testserver und bestehende Repository-Patterns zulässig. Verwende keine
externen Provider oder Internetabhängigkeiten.

## Codequalität und DRY

- Bevorzuge vorhandene Tuliprox-Helper und Modulstrukturen.
- Halte neue APIs crate-intern und schmal.
- Verwende das vorhandene `DynReader`.
- Führe keine `PartialContentPolicy`, keine generische `HttpResponseRepresentationPolicy`, keine zweite Header-Plattform
  und keinen neuen Framework-Layer ein.
- Vermeide positionalen `bool`- oder unklaren `Option`-Parameter in neuen APIs.
- Vermeide Helper, die keine wiederverwendbare Invariante kapseln.
- Halte HLS-spezifische Detection- und Request-Policy klar von generischen Netzwerkhelfern getrennt.
- Aktualisiere Abhängigkeiten und Lockfile atomar.

## Validierung

Führe zuerst die engsten relevanten Tests aus und danach die von `AGENTS.md` verlangten Gesamtprüfungen, soweit im
Repository verfügbar. Erwartet werden insbesondere:

```text
cargo +stable test -p tuliprox content_coding
cargo +stable test -p tuliprox hls
cargo +stable clippy -p tuliprox -- -D warnings
make fmt-check
make lint
make test
```

Falls Markdown-Dokumente geändert werden:

```text
make markdown-lint
```

Melde keinen Befehl als erfolgreich, wenn er nicht ausgeführt wurde oder fehlschlug. Nicht ausführbare Prüfungen müssen
mit exaktem Befehl, Fehlerursache und erfolgreich ausgeführten Ersatzprüfungen dokumentiert werden.

## Abschlussprüfung und Bericht

Validiere die fertige Phase erneut gegen:

1. alle anwendbaren `AGENTS.md`,
2. Phase 1 des Content-Encoding-Konzepts,
3. die dort referenzierten globalen Helper, Invarianten und Tests,
4. die laut Root-`AGENTS.md` bindende allgemeine HLS-Zielspezifikation.

Berichte zusätzlich kurz:

- welche bestehende Decoderlogik ersetzt oder delegierend weiterverwendet wurde,
- an welcher finalen Request-Stelle `identity` tatsächlich durchgesetzt wird,
- wie Deadline, Limit und UTF-8-Validierung zusammenwirken,
- welche Arbeiten bewusst für Phase 2 bis 4 offenbleiben.

Der Abschlussbericht muss exakt mit folgendem Block enden:

```text
Changed files:
New files:
Acceptance criteria satisfied:
Tests / validation performed:
Open points / blockers:
```
