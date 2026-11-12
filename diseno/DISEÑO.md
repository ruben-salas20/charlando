# Charlando: diseño de la interfaz

Maqueta interactiva: `maqueta.html` (abrir en un navegador con WebGL); la del teléfono, `maqueta-android.html`. Cerrado el 2026-11-08; carpetas y permisos añadidos el 2026-11-12.

## Pantallas

1. **Inicio.** Título «Charlando» y el texto «Elige dónde y con quién hablar». Tres bloques y el botón «Empezar»:
   - **Carpeta:** un selector con el nombre y la ruta de la carpeta. Al abrirlo, la lista de carpetas recientes y, al final, «+ Charla nueva». En el teléfono la lista sube como una hoja desde abajo.
   - **Modelo:** Claude Haiku 5.5 (marcado por defecto), Sonnet 5.5 y Opus 5.5.
   - **Razonar:** interruptor, apagado por defecto.
2. **Conversación.** Barra superior: punto de conexión (verde o rojo) junto al nombre de la carpeta, selector «Chat / Voz» en el centro y, a la derecha, el botón de copiar (solo escritorio) y «+».
   - **Chat:** mensajes de «Tú» y «Claude». La respuesta se escribe mientras suena.
   - **Voz:** bola de metal líquido, el estado debajo, tu última pregunta y la respuesta en curso.
   - **Pie:** estado, nivel de audio y la tecla «Espacio» (roja mientras graba). En Voz, solo la tecla.
3. **Tarjeta de permiso.** Aparece cuando Claude necesita permiso para una herramienta:
   - Título «Claude necesita tu permiso» y debajo qué quiere hacer, según `tool`: `Bash` → «Ejecutar un comando»; `Write`, `Edit`, `NotebookEdit` → «Escribir un archivo»; `Task` o `Agent` → «Lanzar un subagente»; los que empiezan por `mcp__` → «Usar un conector»; cualquier otra → «Usar <tool>».
   - El `detail` del protocolo tal cual, en letra de código (IBM Plex Mono), en un recuadro que parte las líneas largas. El recuadro tiene una altura máxima y se desplaza, para que los botones no salgan de la pantalla.
   - Dos botones: «Rechazar» (gris) y «Aceptar» (claro).
   - En Chat va debajo de la última respuesta. En Voz sustituye al texto, y la bola se hace más pequeña.

4. **Abierto en otro dispositivo** (estado `taken` del protocolo). Otra app se ha conectado al cerebro y se ha quedado la conexión:
   - El punto de la barra se pone gris (no rojo: no es un fallo).
   - **Inicio:** encima del botón aparece el texto «Charlando está abierto en otro dispositivo.» y el botón «Empezar» pasa a decir «Usar aquí».
   - **Chat:** el pie muestra «Abierto en otro dispositivo» y, a la derecha, el botón «Usar aquí» en lugar de la tecla Espacio y el nivel de audio.
   - **Voz:** la bola, apagada como en desconectado; debajo, «Abierto en otro dispositivo» y el botón «Usar aquí».
   - En el teléfono, el botón grande de hablar pasa a ser «Usar aquí».
   - «Usar aquí» usa el estilo del botón «Empezar». Al pulsarlo, la app vuelve a conectar y la otra pasa a este estado.

## Comportamiento

- Pulsar para hablar: mantener Espacio graba y soltarlo envía.
- «+» vuelve al inicio y borra la conversación. Cambiar de modelo o de carpeta obliga a empezar de nuevo.
- **Carpeta inicial:** la que se pasa al lanzar la app (`charlando` desde una terminal pasa la carpeta actual). Si no hay ninguna, la más reciente. Si la lista está vacía, «Charla nueva».
- **Rutas:** se muestran absolutas, como llegan del cerebro. Si no caben, se cortan por el principio («…/mi-proyecto»). Debajo de «+ Charla nueva» va «en <charlas>», del `hello`.
- **Barra:** el nombre de la carpeta se corta con «…» si no cabe. Al pasar el ratón se ve la ruta completa.
- **Copiar:** el icono de dos cuadrados copia `cd '<carpeta>' && claude --resume <session>` al portapapeles, para seguir la conversación en la terminal. Solo en el escritorio.
- **Permiso:** la tarjeta se responde solo con el ratón o el dedo. Mientras está abierta, Espacio (o el botón de hablar en el teléfono) no hace nada y se ve apagado. Antes de la tarjeta, Claude dice «Necesito tu permiso. Míralo en pantalla.»; si pasa 1 minuto, lo repite una vez.
- Sin tiempos por etapa en la interfaz, por ahora.
- Vista inicial: por decidir (propuesta: recordar la última y empezar en Chat).

## Bola de metal líquido

Efecto `LiquidMetalParams` de `rune-fx`, preset **gunmetal**. El nivel de voz mueve `flow` y el tamaño.

| Estado | Comportamiento |
|---|---|
| En espera | Casi quieta (`flow` 0,08) y respira despacio (tamaño ±2,5 %) |
| Grabando | Ondas hacia dentro (`speed` negativa), se encoge con tu volumen y la luz se vuelve roja (`light`) |
| Pensando | Barrido de luz: casi quieta (`flow` 0,15) y la luz va y viene (`angle` oscila ±1,1 rad) |
| Hablando | Ondas hacia fuera y crece con el volumen de la voz de Claude |
| Esperando permiso | Como en espera, pero más pequeña, para dejar sitio a la tarjeta. Texto: «Esperando tu permiso» |
| Desconectado | Quieta, más pequeña y apagada (`flow` 0, más contraste) |
| Abierto en otro dispositivo | Igual que desconectado |
