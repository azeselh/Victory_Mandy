# VICTORY AMD ENHANCED

Trabajo local sobre `2119b0e`, en la rama `amd-enhanced-optimization`.
Equipo comprobado: Radeon RX 7900 XT (PCI `1002:744c`), controlador
`32.0.32029.2010`, Windows, Core i5-13400F y 32 GB de RAM según el entorno
aportado. Fecha local de validación: 25 de noviembre de 2026.

Esta versión reduce trabajo repetido de CPU y añade un clamp especializado
experimental para comandos indirectos. Compila en Release. **No se ha
demostrado una mejora de FPS en una escena de juego controlada**, ni que
`VICTORY_RELAY=2` mejore el rendimiento del mundo en esta Radeon.
Las mediciones de funciones aisladas que aparecen abajo no son FPS del juego.

## Referencia recuperable

- La rama original `amd-enhanced` sigue en `2119b0e`.
- Ejecutable original preservado: `target/amd-enhanced-baseline/victory-original.exe`.
- Referencia recompilada antes de editar: `target/amd-enhanced-baseline/victory.exe`.
  Ambas tienen el mismo SHA256, que comienza por `A30B98B7BBF4ED14`.
- Ejecutable modificado: `target/release/victory.exe`.
- El ejecutable del título no se modifica. Las ejecuciones de prueba usan
  copias privadas de partidas y ajustes, fuera del repositorio.
- No se borraron cachés. Las ejecuciones pueden añadir entradas a la caché
  habitual, como cualquier ejecución del programa.
- Los cambios previos del usuario en `.vscode/settings.json` se conservan.

Para volver a ejecutar la referencia basta con usar su ejecutable. No hace
falta revertir archivos ni cambiar de rama. Para desactivar únicamente el
experimento indirecto, dejar sin definir `VICTORY_INDIRECT_SINGLE`, o ponerlo a `0`.

## Arquitectura e investigación

Se revisaron README, la documentación de arquitectura, ray tracing y política
de assets; los manifiestos Cargo del workspace y sus fixtures; toolchain,
configuración local y de ejemplo, configuración Cargo, CI y ajustes del editor.

| Ruta | Responsabilidad y observación |
|---|---|
| `apps/victory`, `victory-core` | CLI, rutas externas, logging y selección de perfil. Release permite logs info, pero no las herramientas Diagnostics. |
| `victory-loader`, `victory-pe`, `victory-hle` | Carga del PE, ejecución x86-64, servicios, archivos y memoria del título. Los remapeos de memoria compiten con lecturas de texturas. |
| `victory-gpu/list.rs`, `queue.rs`, `host.rs` | Registro, cortes por esperas, preparación, submission, fences y copia de resultados. El envío diferido ya existe; no es una nueva implementación de async compute. |
| `victory-gpu/draw.rs`, `indirect.rs` | Bindings, mesh y ExecuteIndirect; clamp GPU no destructivo antes de ejecutar argumentos. Hay ventanas especulativas de argumentos, desactivadas por defecto. |
| `victory-gpu/shadow.rs`, `watch.rs`, `memory.rs`, `inplace.rs` | Copias CPU/VRAM, snapshots, detección de cambios, direcciones del título y heaps sobre memoria existente. Es importante conservar las declaraciones exactas durante vuelos simultáneos. |
| `victory-gpu/resource.rs`, `descriptor/`, `texture.rs` | Recursos, vistas, estados, detiling, staging y coherencia de texturas. La primera transición puede prepararse al enviar la lista. |
| `victory-gpu/pipeline/`, `shader_cache.rs` | Workers de pipelines, traducciones compartidas, caché persistente y pipeline library mapeada. Las traducciones se invalidan al cambiar el ejecutable. |
| `victory-shader` | GFX10, IR SSA, control de flujo, recursos y emisión DXIL, con optimizaciones de módulo ya activas. |
| `victory-formats`, `victory-game`, `victory-pipeline` | Formatos, identificación/contenedores y conversión con caché de contenido. |

Antecedentes relevantes: `e9765a7` desactivó por defecto una forma wave64 con
barreras divergentes que provocaba device loss; `1684989` incorporó los pases
de optimización de shaders; `d279e69` añadió espera configurable y prewarming
de pipelines. No se fuerza wave64 por ser una GPU AMD: eso no corrige barreras
divergentes ni demuestra una mejora en RDNA 3.

La referencia Release ejecutada mostró preparación de texturas, esperas por
remapeos, snapshots y copia de resultados importantes. El recorrido de buffers
bindless incluye también preparación de texturas: su tiempo no puede atribuirse
entero a búsquedas en un hashmap. Se conserva el bloqueo de memoria que impide
leer mientras el runtime remapea páginas. No se eliminan sincronizaciones
basándose en el porcentaje de utilización GPU.

## Cambios implementados

### Declaraciones de escritura repetidas

En [host.rs](../crates/victory-gpu/src/host.rs), la lista recuerda las declaraciones
exactas `(copia, dirección, longitud)` ya registradas. Una repetición no añade
otro `Step::GpuWrite`, no clona otro `Arc` y no repite posteriormente el vaciado
del batch ni la consulta de snapshots. La primera declaración conserva su lugar.

La deduplicación se invalida al registrar un nuevo refresh, una copia de CPU o
trabajo arbitrario de CPU/host. También desaparece al resetear la lista.
Se conservan rangos solapados pero distintos: no se unen ni se cambian las
referencias por vuelo del relay. El test registra 1.000 repeticiones y verifica
las fronteras de refresh, copia, trabajo de CPU y reset. Está activo por defecto.

### Cálculo de bytes pendientes de snapshot

En [shadow.rs](../crates/victory-gpu/src/shadow.rs), `unsnapshotted` resta rangos
sobre un solo vector que crece cuando hace falta. Antes se asignaba otro vector
por cada rango pendiente, incluso sin intersección. Ahora se descartan las
porciones cubiertas y se termina cuando no quedan bytes pendientes.

La unión de bytes copiados es la misma; las porciones son disjuntas y su orden
no afecta a la copia. No se elimina ningún snapshot necesario ni se modifica
su duración. El test usa un oráculo byte a byte, 128 semillas y 64 declaraciones
por semilla, más un extremo del espacio de direcciones. Activo por defecto.

### Clamp de un solo registro

En [indirect.rs](../crates/victory-gpu/src/indirect.rs), la nueva opción
`VICTORY_INDIRECT_SINGLE=1` compila una variante de un hilo cuando la llamada
contiene exactamente un registro. Evita el bucle CAS del presupuesto y los
atómicos de estadísticas mesh, usando cargas/escrituras normales.

La validez depende de que el presupuesto pertenezca a la lista y de las
barreras UAV que rodean el pass. Se conservan ambas barreras globales, todas
las transiciones de scratch, los límites, la saturación del presupuesto y el
tratamiento del count buffer. La variante rechaza internamente más de un
registro; las llamadas múltiples siguen usando el shader original. El modo
Diagnostics que elimina esas barreras no puede seleccionar esta variante.

**Desactivado por defecto.** Se ha probado equivalencia sintética en WARP y
AMD, pero no equivalencia visual durante una partida representativa. Tampoco
se han agrupado dispatches con dependencias desconocidas ni eliminado el clamp.

### Identificación y herramientas

- [main.rs](../apps/victory/src/main.rs): el CLI identifica VICTORY AMD ENHANCED.
- [Benchmark-Victory.ps1](../tools/Benchmark-Victory.ps1): ejecuta un modo por
  proceso, copia partidas, limpia switches heredados en el entorno del hijo,
  restaura el entorno del shell y registra métricas y metadatos fuera del repo.
- [.vscode/tasks.json](../.vscode/tasks.json): tarea `VICTORY Release`.

## Opciones verificadas en el código

| Opción | Estado y semántica real | Uso recomendado en esta entrega |
|---|---|---|
| `VICTORY_RELAY=2` | Implementada. Copia resultados en un hilo independiente y espera solo los vuelos que afectan a la memoria requerida. Sin `VICTORY_LATE_WAIT` explícito implica etapa 3. Incompatible con snapshots conservados; timing GPU desactiva late wait. | Comparación separada pendiente en el mundo. No activada por defecto. |
| `VICTORY_LATE_WAIT=1/2/3` | Implementada. 1 difiere esperas; 2 permite mezcla de escrituras del título bajo vuelos; 3 difiere también present. Conserva puntos de coherencia y límites de listas en vuelo. | Medir cada etapa por separado. |
| `VICTORY_WIDE_BACK=1` | Implementada. Toma snapshots y compara rangos en workers. | Puede afectar al i5 y al ancho de banda; pendiente de A/B. |
| `VICTORY_CARD_BUFFERS` | Activada por defecto; `0` la desactiva. Copias elegibles en VRAM y detección de escrituras en GPU. | Mantener el valor predeterminado. |
| `VICTORY_CARD_BUFFERS_MIN_MB`, `VICTORY_CARD_BUFFERS_MB` | Por defecto 16 MiB y 512 MiB. El mínimo se limita al menos a 1 MiB. | El benchmark ofrece bajar solo el umbral a 1 MiB; no aumenta a la vez el presupuesto. |
| `VICTORY_CARD_WINDOWS_MB` | 0 por defecto: no promociona esas ventanas a VRAM. | Mantener. |
| `VICTORY_KEEP_SNAPSHOTS` | Conservación experimental; desactivada. Presupuesto predeterminado 256 MiB. | Mantener desactivada. |
| `VICTORY_INPLACE` | Ruta experimental para usar memoria del título mediante heaps, con limitaciones de coherencia documentadas en el código. | No activada. |
| `VICTORY_INDIRECT_WINDOW` | Ventanas de argumentos preparadas anticipadamente; desactivada. Puede cargar presupuesto por registros que después no se dibujan. | No activada. |
| `VICTORY_INDIRECT_SINGLE` | Nueva variante descrita arriba; desactivada. | Solo prueba individual. |
| `VICTORY_SHADER_OPT` | Pases de optimización implementados y activos; el valor participa en la clave de caché. | Mantener. |
| `VICTORY_LIBRARY_MAP`, `VICTORY_LIBRARY_HANDLE`, `VICTORY_LIBRARY_TRIM` | Library mapeada, handle conservado y descarte de páginas implementados, activos por defecto. | Mantener. |
| `VICTORY_PIPELINE_WAIT_MS` | Presupuesto por frame para esperar pipelines, 500 ms por defecto; reducirlo puede omitir draws hasta que estén disponibles. | No reducirlo para fabricar una ganancia de FPS. |

No existe una opción global previa llamada AMD ENHANCED. No se han cambiado
la traducción de shaders, la identidad de sus cachés ni la política de pipelines.
La reconstrucción cambia el hash del ejecutable y por ello calienta otra caché
de traducciones: cada binario debe calentarse por separado antes del A/B.

## Mediciones

### CPU aislada, compilación Release

Cada resultado es la mediana de cinco rondas de 10.000 llamadas, alternando
el orden original/modificado. El test incorpora la implementación anterior y
comprueba igualdad de resultados. No había juego ni compilación concurrentes.

| Distribución sintética de 128 rangos | Original, µs/llamada | Modificado, µs/llamada |
|---|---:|---:|
| Solapados | 4,126 | 0,323 |
| Separados | 35,999 | 6,541 |
| Ya cubiertos | 3,815 | 0,047 |

El caso cubierto gana además por terminar anticipadamente. Estas distribuciones
no están extraídas de una captura del título; no permiten calcular ahorro por frame.

### GPU aislada, compilación Release

`benchmark_single_record_clamp` usa timestamps de D3D12 en la Radeon
`1002:744c`: 4.096 clamps por muestra, seis rondas alternando orden, con las
mismas barreras para ambas variantes. Mide el clamp, **no ExecuteIndirect ni el
shader del juego**. El resultado de mesh después de la primera ronda estuvo
entre 1,641 y 1,718 µs/clamp para el original, y entre 1,614 y 1,707 para la
variante. Dispatch mostró más calentamiento, desde más de 3 µs al comienzo.
Hay parejas con ligera regresión; no se considera ganancia estable del juego.

### Juego, referencia Release

Muestra no controlada de 30,39 segundos, 16 muestras de proceso y cinco ventanas
del logger, entre `23:08:43` y `23:09:14` UTC del 24 de noviembre
(25 de noviembre en Madrid). No se confirmó una cámara/escena fija.

| Ventana | FPS | ms/frame | Preparación CPU, ms | Espera CPU de GPU, ms | Copia de vuelta, ms |
|---|---:|---:|---:|---:|---:|
| 1 | 1,9 | 539,6 | 247,1 | 164,2 | 87,7 |
| 2 | 2,3 | 436,6 | 182,7 | 154,4 | 90,5 |
| 3 | 2,4 | 409,9 | 160,8 | 155,8 | 85,4 |
| 4 | 1,3 | 773,2 | 466,5 | 166,9 | 126,9 |
| 5 | 0,8 | 1.207,7 | 757,8 | 179,4 | 132,5 |

La preparación de texturas osciló de 7,5 a 512,7 ms en esas ventanas.
La muestra demuestra variabilidad y trabajo de CPU; no demuestra que todos
esos costes provengan de la función de snapshots modificada.

El logger original llama «on the GPU» al tiempo de espera de CPU; se etiqueta
correctamente en el CSV. El contador `indirect` tampoco cubre todas las rutas
mesh/dispatch. No se presentan estos contadores como censo completo de comandos.
Los contadores Windows de memoria GPU son memoria dedicada/compartida del
proceso; el máximo entre motores GPU no es utilización total del adaptador.

**Pendiente:** FPS antes/después del mundo, tiempo real GPU por frame, censo
completo de barreras/indirectos y tiempos comparables de traducción/creación
de pipelines. No se reutilizan los tiempos de Diagnostics aportados por el
usuario como referencia de FPS final.

## Validación y límites

- `cargo build --release -p victory`: compila, sin `diagnostics`.
- `cargo test --workspace --locked`: 500 tests correctos, ninguno fallido;
  los tests ignorados incluyen pruebas manuales y de datos externos.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pasa.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: pasa.
- Clamp: 240 casos por dispositivo (WARP y AMD), incluyendo cuatro tipos de
  argumento, datos extremos, count cero, campos extra y presupuesto agotado.
  Comparación exacta de argumentos y presupuesto, fuente inalterada.
- La prueba focalizada con `diagnostics` y `VICTORY_D3D12_DEBUG=1` habilitó
  la capa D3D12 y no encontró mensajes de error. No es una medición de FPS.
- Microbenchmarks CPU y GPU ejecutados en Release; separados de los tests Debug.
- El primer Clippy terminó correctamente pero Windows informó que no pudo
  finalizar algunos directorios incrementales (`Acceso denegado`); no fueron
  errores del código ni se borraron esos directorios.
- El cierre normal de la instancia de referencia no terminó el proceso;
  se terminó únicamente ese PID tras verificar la ruta del ejecutable de
  referencia. Esa instancia usaba datos temporales.

Los defectos gráficos previos, ray tracing incompleto, draws sin pipeline y
limitaciones de coherencia ya documentadas por el fork siguen siendo trabajo
pendiente. No se certifica corrección visual ni estabilidad prolongada mediante
tests sintéticos. No se han eliminado comprobaciones para aumentar los FPS.

## Ejecución y comparación repetible

Desde la raíz del repositorio, con `$gameExe` apuntando al ejecutable original:

```powershell
cargo build --release -p victory
./target/release/victory.exe --version
./target/release/victory.exe run $gameExe --execute
```

Para medir sin tocar tus partidas habituales, cierra otras instancias y usa
el mismo `DataSeed` en todas las ejecuciones. El script hace una nueva copia
cada vez. Mantén la misma resolución, cámara, partida y aplicaciones de fondo;
espera a que terminen compilación y streaming, y escribe `READY` en el prompt.
Después de cada muestra cierra el juego normalmente.

```powershell
# Referencia original.
./tools/Benchmark-Victory.ps1 -TitleExecutable $gameExe -BuildKind Release `
  -Executable ./target/amd-enhanced-baseline/victory.exe -Seconds 60

# Modificada, sin opciones experimentales.
./tools/Benchmark-Victory.ps1 -TitleExecutable $gameExe -BuildKind Release -Seconds 60

# Únicamente relay=2 sobre la modificada.
./tools/Benchmark-Victory.ps1 -TitleExecutable $gameExe -BuildKind Release -Mode relay2 -Seconds 60

# Únicamente el nuevo clamp sobre la modificada.
./tools/Benchmark-Victory.ps1 -TitleExecutable $gameExe -BuildKind Release -Mode indirect-single -Seconds 60
```

Repetir al menos tres veces, alternando A/B/B/A y calentando ambos binarios
antes de medir. Los modos `wide-back`, `late-wait3` y `card1mb` permiten
investigar esas opciones de una en una. No se acumulan con `relay2`.

Cada carpeta externa contiene `metadata.json`, `process.csv`, `frames.csv`,
logs y su propia copia de datos. Los números CSV usan la configuración regional
del PowerShell que los creó. Las filas de frames son ventanas muestreadas del
logger: no sirven para calcular percentiles de tiempos de frames individuales.

Reproducir los microbenchmarks sin un juego ni compilación simultáneos:

```powershell
cargo test --release -p victory-gpu benchmark_snapshot_ranges -- --ignored --nocapture
cargo test --release -p victory-gpu benchmark_single_record_clamp -- --ignored --nocapture
```

La configuración recomendada actualmente para la RX 7900 XT es **Release con
los valores predeterminados** de este fork y las dos optimizaciones CPU activas.
Solo adoptar relay, el nuevo clamp u otra opción después de un A/B de la misma
escena con validación visual. No se recomienda forzar wave64, omitir barreras,
reducir límites de shaders ni acortar la espera de pipelines para mejorar cifras.
