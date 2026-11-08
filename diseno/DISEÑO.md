# Charlando: diseño de la interfaz

Maqueta interactiva: `maqueta.html` (abrir en un navegador con WebGL). Cerrado el 2026-11-08.

## Pantallas

1. **Inicio.** Título «Charlando» y la lista de modelos: Claude Haiku 5.5 (marcado por defecto), Sonnet 5.5 y Opus 5.5. Botón «Empezar».
2. **Conversación.** Barra superior: punto de conexión (verde o rojo) junto a «Charlando», selector «Chat / Voz» en el centro y botón «+» a la derecha.
   - **Chat:** mensajes de «Tú» y «Claude». La respuesta se escribe mientras suena.
   - **Voz:** bola de metal líquido, el estado debajo, tu última pregunta y la respuesta en curso.
   - **Pie:** estado, nivel de audio y la tecla «Espacio» (roja mientras graba). En Voz, solo la tecla.

## Comportamiento

- Pulsar para hablar: mantener Espacio graba y soltarlo envía.
- «+» vuelve al inicio y borra la conversación. Cambiar de modelo obliga a empezar de nuevo.
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
| Desconectado | Quieta, más pequeña y apagada (`flow` 0, más contraste) |
