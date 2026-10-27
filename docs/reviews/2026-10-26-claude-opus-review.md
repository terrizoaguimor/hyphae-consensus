# Auditoría independiente: Hyphae Consensus

No modifiqué nada en el repositorio. Tampoco ejecuté `cargo test`, porque la revisión era de solo lectura. Lo único que escribí fue un resumen de recomendaciones en mi archivo de plan, fuera del repo.

## Veredicto

El núcleo está bien pensado y es mejor que el de la mayoría de proyectos de hackathon: contratos tipados, una sola función pura que decide, log append-only y una documentación inusualmente franca sobre los límites. Pero la frase "los modelos nunca deciden" se cumple solo en la forma. En la práctica el Evaluator decide casi todo, y la política tiene dos agujeros que dejan salir `Supported` cuando no debería. Lo que más puntos pone en riesgo: todo el flujo estrella (challenge → revisión → Supported) es scripted, las corridas live abstienen y la evaluación es circular.

## Fortalezas

- **Las salidas del modelo no tienen campos de resultado.** `consensus-domain/src/proposal.rs` y los drafts de `meta.rs` (`DraftProposal`, `DraftEvaluation`…) no tienen campos de outcome ni de autorización. Los ids los asigna el adaptador.
- **Una sola función decide la recomendación.** `policy.rs:derive_recommendation` es pura, tiene prioridades claras y tests unitarios razonables.
- **Integridad, verdad y relevancia son tres ejes separados en los tipos** (`evidence.rs`). Además `RunReceipt::DEFAULT_LIMITATIONS` siempre viaja en el receipt.
- **El event log está bien hecho para su tamaño** (`event_log.rs:open`):
  - valida secuencias sin huecos;
  - distingue una cola truncada (recuperable) de una corrupción a mitad del archivo (error duro);
  - detecta `event_id` duplicados ya guardados;
  - hace fsync en cada append.
- **Hay limpieza defensiva en el adaptador Meta:**
  - descarta ids desconocidos (`meta.rs:critique` y `evaluate`);
  - completa como *no corroborado* cualquier claim que el Evaluator omita (`meta.rs` ~L424);
  - un JSON mal formado hace fallar la corrida en lugar de dejarla avanzar.
- **Los errores del adaptador Hyphae se tratan como errores.** Un error de herramienta dentro de `structuredContent` se trata como error y no como datos (`hyphae.rs:call_tool`). El cliente se niega a operar si el servidor no ofrece la herramienta de recall, y nunca pasa `--allow-write`.
- **La documentación es honesta en casi todo:** declara `provider:"fake"`, separa claramente el trabajo previo de Hyphae del nuevo, y en `evaluation.md` reconoce que no repitió corridas buscando un `supported`.

## Debilidades y riesgos (de más a menos grave)

### 1. Agujero en la política: una contradicción no impide `Supported` (correctitud y seguridad)

En `policy.rs:derive_recommendation` (L30–33 y L44–50), las reglas de *Blocked* y *Contested* solo miran la evidencia que el Evaluator puso en `relevant_evidence`.

- **Escenario:** el claim c1 viene con `corroborated=true`, `relevant=[ev1]` y `contradicting=[ev2]`. El orquestador sí guarda `ev2` como `Contradicted/Relevant` (`orchestrator.rs` L189–214), pero la política no la ve. Resultado: **Supported con una contradicción registrada en el mismo log**.
- **Lo mismo con una prueba Invalid:** si la evidencia con prueba inválida no se cita como relevante, no bloquea. El Evaluator decide, por omisión, si la corrupción importa.

### 2. Un claim sin evidencia puede salir "corroborado"

`relevant.is_empty()` se comprueba de forma global (L60–63), no por claim.

- **Escenario:** c1 corroborado con `[ev1]` válido y c2 corroborado con `[]`. Resultado: **Supported**, aunque c2 no tiene ningún respaldo.
- El README afirma que "a `supported` requires corroborated claims *and* valid proofs on every cited record". Eso es literalmente cierto, pero engañoso: si el claim no cita nada, no hay nada que fallar.

### 3. "Los modelos no deciden" es cierto solo en la forma

La recomendación es una agregación determinista de booleanos escritos por un modelo (el Evaluator):

- `corroborated` lo pone el Evaluator;
- `unresolved_challenges` lo pone el Evaluator (si devuelve `[]`, se borra el Contested);
- la relevancia la pone el Evaluator.

El Critic no tiene ningún efecto mecánico: sus ítems no entran en la política. Si un Evaluator manipulado o complaciente marca todo como corroborado sobre evidencia con prueba válida, el resultado es **Supported**. Esto es defendible como diseño, pero el pitch y el README deben decirlo así, no "models never decide outcomes".

### 4. La inyección vía memoria no está contenida por código y el test es teatro

- `injected_instruction_in_memory_is_data_not_authority` usa un `FakeModelClient` que ni siquiera lee el contenido; el resultado lo fija el script (`corroborated=false`). No prueba nada sobre inyección. Lo mismo pasa con el caso del evaluador baseline.
- **Fuga por el delimitador:** `meta.rs:evidence_block` no escapa el texto. Un recuerdo que contenga `</UNTRUSTED_EVIDENCE>` cierra el bloque y lo que sigue llega fuera del "sandbox textual".
- **Canal lateral sin delimitar:** la salida del Proposer (claims, resumen) entra sin delimitar en los prompts del Critic y del Evaluator (`proposal_text`). Un Proposer inyectado puede dar órdenes directamente al Evaluator, que es el rol que decide.
- **Una prueba válida no protege contra esto:** un recuerdo malicioso guardado legítimamente en Hyphae tendría prueba **Valid**. La integridad no sirve contra la inyección, y el único freno real es la obediencia del modelo.

### 5. La consola sustituye modos en silencio (contradice la ADR-0004)

- `server.rs:202`, `("fake", _)`: con `model=fake`, el selector de memoria **se ignora**. "fake + offline" o "fake + hyphae" ejecutan el escenario Supported sin avisar. La ADR-0004 dice "Nothing silently substitutes one mode for another".
- `server.rs` L233/L243: `meta + fake` y `meta + offline` usan `FakeProofVerifier::all(Valid)`. La "corrida live" de Meta citada en `evaluation.md` trabajó con pruebas válidas falsas, aunque el campo `verifier` dice `fake-verifier`.
- La "failure gallery" del paso 6 de `demo-script.md` (offline / prueba inválida / inyección) **no se puede producir desde la consola**. Prueba inválida e inyección solo existen en tests y en `evaluate`.

### 6. Límite de rondas: la segunda revisión nunca ocurre en modo live y no hay test que lo cubra

- `meta.rs` L433–438 filtra `unresolved_challenges` a `== challenge.id`. Como hay un solo documento de challenge por ronda, unresolved solo puede valer 0 o 1.
- En `orchestrator.rs:234` la regla de "sin progreso" es `unresolved >= prev`. Ronda 1: unresolved=1, se revisa. Ronda 2: unresolved=1, sin progreso, se para. **Con Meta, `MAX_REVISION_ROUNDS=2` nunca se alcanza.**
- Ningún test llega a `revision_round == 2` ni dispara `TransitionError::RoundLimit`.
- Los ítems del challenge no tienen id, así que no se pueden resolver uno a uno.

### 7. La revisión trabaja a ciegas

`meta.rs:revise` (L460–475) solo pasa las notas del Evaluator y el id del challenge. **No pasa los ítems del Critic ni el texto de la evidencia**, así que el Proposer no ve la restricción "cap = 3 con jitter" que debía corregir. El flujo estrella del demo no puede reproducirse en live salvo por suerte. Probablemente esto contribuye a que las corridas reales terminen abstenidas.

### 8. Un log "auditable" que no detecta manipulaciones y que atribuye mal los actores

- No hay hash-chain ni firma. Si alguien edita `events.jsonl` respetando las secuencias (por ejemplo, cambiando `kind` a `supported`), el replay lo acepta: `RecommendationIssued` no se recalcula a partir de la evaluación. Para un proyecto llamado "Verifiable … Accountability" es una brecha visible para un juez técnico.
- `orchestrator.rs` L103–106 y L121: `EvidenceRetrieved` y `EvidenceUnavailable` se registran como `Actor::Agent{EvidenceEvaluator}`, pero lo hace el runtime. El pitch repite el error ("Evidence Evaluator — retrieves records… verifies sealed integrity proofs"). El modelo no tiene ninguna capacidad de recuperar ni de verificar.

### 9. "Crash-safe" protege el archivo, no la corrida

- **Corridas huérfanas:** si el proceso muere a mitad de una corrida, esta queda para siempre en `EvidenceReview`. No hay reanudación ni marca de "abandonada" al arrancar. `interrupt_reopen_and_continue_to_decision` "interrumpe" **después** de terminar la corrida, así que no prueba un crash real.
- **Escritura parcial que inutiliza todo el log:** en `event_log.rs:append` (L184–198), si `write_all` falla a medias (disco lleno), no se trunca hacia atrás. El siguiente append se escribe detrás de la basura. Al reabrir, eso es corrupción a mitad del archivo y deja ilegibles **todas** las corridas.
- **Fallos de log en silencio en la consola:** `server.rs:snapshot_events` y `aggregates` descartan en silencio líneas malas y errores de transición (`let _ = agg.apply`). La consola puede mostrar un estado distinto del real.

### 10. La aprobación humana es una etiqueta, no una autenticación

- `state_machine.rs` L171–183 comprueba `Actor::Human`, pero ese actor lo construye `record_human_decision` a partir de un `authorized_by` de texto libre que llega por HTTP sin autenticar.
- Cualquier proceso local puede hacer `curl POST /decision approved=true`, incluido un agente de código con shell, que es justo el usuario objetivo del pitch.
- Se puede aprobar una corrida **Blocked** sin pedir una nota de override.
- La clave de idempotencia es global, no por corrida (`orchestrator.rs:326`, `human-decision:{key}`). Si se reutiliza una clave en otra corrida, se devuelve 200 sin registrar nada. Y la UI genera un UUID nuevo en cada clic (`console.html:42`), así que en la práctica la idempotencia no se usa.

### 11. Hyphae: el "anclaje" sale de la misma fuente no confiable y hay una degradación peligrosa

- `hyphae.rs:map_record` (L297) toma el anchor de la **misma respuesta** que entrega la prueba. Verificar contra un ancla que proporciona el propio servidor solo demuestra consistencia interna. Hace falta un ancla obtenida por otra vía (fijada por el operador o tomada antes). Conviene confirmarlo contra el modelo de confianza documentado por Hyphae.
- `recall` (L316–326) reintenta **sin prueba ante cualquier error**, incluido el error actual, que es literalmente `corruption: native durable state is invalid`. Una señal de corrupción acaba como `Unavailable`/InsufficientEvidence en vez de Blocked, y el log no guarda la razón.
- `verify` (L407): cualquier código de salida distinto de 0 se mapea a `Invalid`, y por tanto a Blocked. Un `docker exec` caído (códigos 125–127) o un error de uso producirían **falsos Blocked**.
- Live, las pruebas fallan, así que ninguna corrida con Hyphae real puede llegar nunca a `Supported`. La demo criptográfica es imposible hoy, y la ADR-0003 lo reconoce.

### 12. La evaluación baseline vs consenso es circular

- En `evaluate.rs`, el baseline **no se ejecuta**: `initial_unsafe: true` es una constante que escribió el autor.
- El "consenso seguro" se mide con predicados escritos para resultados que el mismo autor guionizó. "6/7 vs 7/7" mide que `derive_recommendation` implementa las reglas que dice implementar, no que el protocolo mejore decisiones.
- El caso `model-timeout` marca el baseline como "UNSAFE accept" (L200), pero un baseline sin propuesta no acepta nada. Es un número inflado.
- La documentación sí aclara "fake mode", pero el pitch y el demo repiten el 6/7 como titular.

### 13. Documentación desfasada y afirmaciones de más

- La ADR-0002 dice que el test live "has not been run" y la ADR-0003 que "no Hyphae runtime installed". Ambas cosas son falsas ya.
- `mission-control.md` dice que `memory=hyphae` está bloqueado y que **la UI no se ha probado contra el servidor Rust**. Esa UI es justo lo que va a salir en el video.
- El párrafo de estado del README tiene una frase rota ("Upstream Windows 4.0.0 Agent Memory surface, which fails to start…").
- ADR-0002 punto 1: `strict: false` (`meta.rs:178`). La validación real la hace serde y no la API, lo cual está bien, pero hay que decirlo.
- El README dice que, en modo offline, la evidencia "carries `integrity_status: unavailable`". En realidad no se crea ninguna verificación: el tope sale de `relevant.is_empty()`.
- La licencia está "undecided". Revisa si las reglas del hackathon exigen repo público con licencia OSI: podría ser descalificante.

### 14. Gastos sin límite real

El número de recalls y de procesos de `proof verify` crece con el número de claims, que controla el modelo. No hay tope de claims, ni de tamaño de evidencia en el prompt, ni presupuesto de tokens por corrida. La amenaza 5 del threat model ("model/memory calls are bounded per run") es cierta para las llamadas al modelo (máximo 9), no para la memoria.

## Rúbrica: dónde se pierden puntos

| Criterio | Riesgo | Por qué |
|---|---|---|
| **Demo (25%)** | **Alto** | El flujo estrella solo existe con datos scripted. La galería de fallos no se puede mostrar en la consola. La UI de Mission Control no se ha probado contra el backend. Las corridas live abstienen. |
| **Meta AI Tooling (25%)** | **Alto** | Un solo modelo, por una llamada genérica de chat completions con `json_schema`. Hay evidencia de 1–2 corridas reales, ambas abstenidas. No hay ninguna métrica de que el protocolo mejore la salida de Meta. La participación es real pero poco *demostrada*. |
| **Impacto (30%)** | **Medio-alto** | Un sistema que en live siempre se abstiene parece inútil. Hace falta mostrar que detecta la violación de la restricción *de verdad* y que con una buena revisión llega a Supported. |
| **Profundidad técnica (20%)** | **Medio** | La arquitectura es sólida. Un juez atento encontrará los puntos 1, 2 y 8 en minutos. |

## Top 5 recomendaciones antes del 4 de noviembre

1. **Cerrar los agujeros de la política (≈½ día).**
   - En `derive_recommendation`, considerar relevante la unión de `relevant_evidence` y `contradicting_evidence`.
   - Exigir que cada claim corroborado tenga al menos una evidencia `Valid` propia.
   - Bloquear con *cualquier* prueba Invalid entre la evidencia recuperada.
   - Añadir tres tests adversariales con exactamente los escenarios de los puntos 1 y 2, y uno que llegue a `revision_round == 2` y a `RoundLimit`.
2. **Que los modos sean honestos y la demo se pueda reproducir (≈1 día).**
   - Rechazar con 400 las combinaciones `fake + ≠fake`, o tratar el modo de memoria como parámetro real.
   - Etiquetar `meta + fake` como "pruebas simuladas" en la UI y en el receipt.
   - Añadir escenarios seleccionables desde la consola (offline / prueba inválida / inyección).
   - Probar Mission Control contra el servidor real antes de grabar.
3. **Hacer sustantiva la participación de Meta (≈1–2 días).**
   - Pasar a `revise` los ítems del Critic y la evidencia (delimitada y escapada).
   - Dar id a cada ítem de challenge y medir el progreso por ítem.
   - Escapar `</UNTRUSTED_EVIDENCE>` y delimitar también la salida del Proposer.
   - Lanzar **N ≥ 10 corridas live** sobre 3–4 tareas sembradas (incluida una con recuerdo malicioso *real*), con request ids y la tabla de resultados tal cual salga.

   Esa tabla, aunque sea modesta, vale más que el 6/7 scripted.
4. **Que la auditoría sea verificable (≈½ día).**
   - Hash-chain (`prev_hash`) en cada evento y el hash final en el receipt.
   - En el replay, recalcular `derive_recommendation` y fallar si no coincide con lo registrado.
   - Actor `Runtime` en `EvidenceRetrieved`.
   - Truncar hacia atrás si una escritura queda a medias.
   - Al arrancar, marcar como `RunFailed{stage:"interrupted"}` las corridas no terminales.
   - Token aleatorio, impreso al arrancar, obligatorio en `/decision`; exigir nota para aprobar una corrida Blocked o Abstained.
5. **Poner la documentación al nivel del código (≈½ día).**
   - Actualizar las ADR-0002 y 0003, `mission-control.md` y el párrafo de estado del README.
   - En el pitch: "el runtime recupera y verifica; el Evaluator juzga".
   - Recalificar el test de inyección como "test de política" y quitar el "UNSAFE" del timeout.
   - Declarar sin rodeos que la demo criptográfica de Hyphae no está disponible y que el ancla viene del mismo servidor.
   - Decidir la licencia según las reglas del concurso.

Si solo puedes hacer dos: la 1 (cualquier juez técnico encuentra esos agujeros) y la 3 (es lo que convierte "usa Meta" en "Meta hace algo demostrable").
