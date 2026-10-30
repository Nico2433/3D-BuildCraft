# User interface

Observed in 25 screenshots of the running app (Spanish UI, 1920×1080, reference photos kept outside the repository). English names are the matching resource labels where known. Findings H46–H48 and H49–H56 in [Findings](findings.md).

## Main window

- Title bar: `3D Builder - <scene name>`.
- Top bar (blue): hamburger menu, tabs **Insertar, Objeto, Editar, Pintar, Ver, Ayuda**; on the right **Impresión 3D**, **Guardar**, **Deshacer**, **Rehacer**.
- Ribbon (dark): the commands of the active tab, as icon + label.
- Viewport: checkerboard floor with distance labels (50 mm, 100 mm…), soft shadow and floor reflection. The selected object gets a highlight outline and move gizmos (blue vertical arrow for height, green horizontal arrow).
- Bottom-centre transform bar: mode buttons **move / rotate / scale**, numeric **X Y Z** fields and the unit (`mm`).
- Right collapsible pane **Selección**: Grupo, Desagrupar, Selección total, Sin selección, Invertir selección, Selección especial, and an **Elementos** list with one thumbnail per object.

## Tabs

| Tab | Commands |
|---|---|
| Insertar | Agregar, Personalizado, Cubo, Cilindro, Pirámide, Cono, Esfera, Hexágono, Cuña, Toro, Tetraedro |
| Objeto | Duplicar, Copiar, Cortar, Pegar, Eliminar, Instalar (puts the object on the floor; odd translation), Reflejo (mirror), Medida (measure) |
| Editar | Simplificar, Dividir (plane cut), Suave (smooth), Relieve (emboss), Extrudir hacia abajo, Combinar (merge), Intersecar, Restar (both disabled with one object), Ahuecar (hollow) |
| Pintar | Material, Color, Textura |
| Ver | Vista central; toggles Sombreado, Sombras, Colores, Reflejos, Suavizando; Estructura metálica (wireframe), Cuadrícula (grid), Rayos X (x-ray) |
| Ayuda | Introducción, Impresoras compatibles, Guía del usuario |

The **Ver** toggles map onto the `BuilderRender*` settings in [Data model](data-model.md).

## Hamburger menu

Nueva escena, Abrir, Guardar, Guardar como; Impresión 3D, Pedir impresión en línea, Imprimir en papel; Acerca del modelo, Estado de la impresora 3D; Comentarios, Configuración, Calificar esta aplicación.

**Configuración** panel:

- Unidades: µm, mm, cm, in, pie (ft), m.
- Modo de color de la aplicación: system / light / dark.
- Ajustar (snapping): on. Colisión (collision): off.
- Version line `3D Builder 20.0.4.0 / Microsoft Corporation`, plus links to compatibility info and the privacy statement.

## Dialogs and flows

**Agregar (Add)**: left column **Recientes** (thumbnail grid, "más"), then **Cargar objeto**, **Cargar imagen**, **Cámara**. Right side **Biblioteca 3D** with a search box and categories *Cocina*, *Miniaturas y figurillas*, *Kit de trofeo*. Those categories differ from the packaged catalog, suggesting remote content in the captured session; the tile images were empty.

**Import**: opening a file shows a full-screen preview with a contextual bar **Cancelar / Importar modelo** and **Unidades del archivo** (µm, mm, cm, in, pie, m; mm preselected).

**Repair prompt**: after import, an invalid mesh gets a red bounding box and a toast at the bottom right: a notice explaining that some objects are invalid and offering repair. Repair is started from that toast (behaviour measured in H35).

**Tool mode pattern**: tools such as Color, Material and Textura replace the top bar with a contextual bar: **Cancelar**, **Aplicar** (or the tool name), the tool options, and their own **Deshacer / Rehacer**. An info toast explains the tool at the bottom right.

| Tool | Options |
|---|---|
| Material | Tipo (`Simple`, …), Color swatch, Seleccionar material (eyedropper); applies to the whole object |
| Color | Seleccionado (colour swatch), Cobertura slider (brush coverage); click on the surface to paint |
| Textura | Seleccionado (texture, e.g. wood), modes **Adhesivo** (sticker) and **Pincel** (brush), Cobertura slider |

### Modelling tools

| Tool (tab) | Contextual bar | Viewport |
|---|---|---|
| Personalizado (Insertar) | Estructura metálica toggle; Forma (`Cubo`, …); Latitud 10; Longitud 10; Ancho 40 mm; Altura 40 mm; Redondez 0; Agregar | Grey preview of a subdivided primitive with wireframe; the scene is shown translucent behind it |
| Relieve (Editar) | Patrón (`A` = text, or shapes); Proyección (`Planar`, cylindrical, spherical); Bisel toggle; Color: Superior and Lateral pickers; Texto; Fuente (`Segoe UI`); Relieve to apply | Text outline projected onto the surface with move/rotate handles; the bottom bar switches to scale in % (X 20, Y 20, Z 50) with a lock for uniform scale |
| Simplificar (Editar) | Nivel de reducción slider; Número de caras and Número de caras originales (592 / 592); Reducir caras | Mesh shown with wireframe |
| Dividir (Editar) | Mantener parte superior / inferior / ambos; Dividir | Green cutting plane with a rotation sphere and arrows; bottom bar with move/rotate mode and an offset in mm |
| Ahuecar (Editar) | Grosor de pared mínimo slider, value shown (1 mm); Ahuecar | Object shown in x-ray so the inner wall is visible |
| Medida (Objeto) | Distancia (0.00 mm); Ajustar (snap) toggle; only Cancelar | A point marker on the surface; click two points |

### Print preparation (Impresión 3D)

A full-window view with a left navigation (**Impresora, Material, Diseño, Estado de la impresora 3D, Acerca de**), a preview on the right with rotate, zoom in/out, edit and "…" buttons, and a bottom bar: **Costo estimado** (here "No disponible"), a warnings counter ("1 advertencia"), Deshacer, Rehacer, **Pedir en línea** and **Cancelar**.

| Page | Content |
|---|---|
| Impresora (Selección de impresora) | "Servicio de impresión 3D en línea" tile; **Impresoras instaladas** (here `None`, untranslated); **Impresoras detectadas** with an **Actualizar** button |
| Material | Tipo de material (empty dropdown), Seleccionar acabado y color. A toast reported an unspecified print-service error and suggested retrying: the online print service did not answer |
| Diseño | Unidades (`Milímetro`), Escala (100%), Ancho 120.005 mm, Alto 59.9688 mm, Profundidad 40.001 mm, **Rest. orientación**, **Restabl. escala**; the preview shows a bounding box with rotation sphere |
| Estado de la impresora 3D | Opens as a separate window "Estado de la impresora 3D - 3D Builder" with a refresh button and an empty-job message |

With no printer installed, no slicer settings (layer height, infill, supports) were shown; those pages need a configured 3D printer.

## Not captured

Slicer settings for a real printer, the Cámara capture, Extrudir hacia abajo, Suave, the image-based emboss and the Acerca del modelo panel.
