//! UI languages. Text is written in English in the code and `tr` returns it in the
//! chosen language.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Lang {
    Es,
    #[default]
    En,
}

impl Lang {
    pub fn system() -> Self {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetUserDefaultLocaleName(name: *mut u16, length: i32) -> i32;
            }
            let mut name = [0_u16; 85];
            // The buffer includes the terminating null and is large enough for a locale name.
            let length = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
            if length > 1 {
                return Self::from_locale(&String::from_utf16_lossy(&name[..length as usize - 1]));
            }
        }
        let locale = environment_locale(|key| std::env::var(key).ok());
        #[cfg(target_os = "macos")]
        let locale = locale.or_else(|| {
            std::process::Command::new("defaults")
                .args(["read", "-g", "AppleLocale"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        });
        Self::from_locale(&locale.unwrap_or_default())
    }

    pub const ALL: [Lang; 2] = [Lang::Es, Lang::En];

    /// Shown in its own language so anyone can find theirs.
    pub fn label(self) -> &'static str {
        match self {
            Lang::Es => "Español",
            Lang::En => "English",
        }
    }

    pub fn from_locale(tag: &str) -> Lang {
        if tag.to_ascii_lowercase().starts_with("es") {
            Lang::Es
        } else {
            Lang::En
        }
    }
}

fn environment_locale(mut get: impl FnMut(&str) -> Option<String>) -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| get(key).filter(|locale| !locale.is_empty()))
}

/// `en` in `lang`; text without a translation stays in English.
pub fn tr(lang: Lang, en: &'static str) -> &'static str {
    match lang {
        Lang::En => en,
        Lang::Es => spanish().get(en).copied().unwrap_or(en),
    }
}

fn spanish() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    MAP.get_or_init(|| ES.iter().copied().collect())
}

const ES: &[(&str, &str)] = &[
    // Tabs and top bar
    ("Insert", "Insertar"),
    ("Invert", "Inverso"),
    ("Coverage", "Cobertura"),
    ("Add library folder", "Agregar carpeta de biblioteca"),
    ("Add a folder with 3D models to use it as a library.", "Agrega una carpeta con modelos 3D para usarla como biblioteca."),
    ("Remove library", "Quitar biblioteca"),
    ("No models here.", "No hay modelos aquí."),
    ("Right-click a library to remove it.", "Haz clic derecho en una biblioteca para quitarla."),
    ("Contour", "Contorno"),
    ("Height map", "Mapa de alto"),
    ("Edge", "Borde"),
    ("Stamp", "Sello"),
    ("Method", "Método"),
    ("Levels", "Niveles"),
    ("Textures", "Texturas"),
    ("Import image", "Importar imagen"),
    ("Scale factor", "Escala"),
    ("Shape", "Forma"),
    ("Latitude", "Latitud"),
    ("Longitude", "Longitud"),
    ("Width", "Ancho"),
    ("Roundness", "Redondez"),
    ("No recent files", "No hay archivos recientes"),
    ("Load object", "Cargar objeto"),
    ("Load image", "Cargar imagen"),
    ("Camera", "Cámara"),
    ("3D library", "Biblioteca 3D"),
    ("Search", "Buscar"),
    ("Estimated cost:", "Costo estimado:"),
    ("Not available", "No disponible"),
    ("Order online", "Pedir en línea"),
    ("Printer", "Impresora"),
    ("Design", "Diseño"),
    (
        "objects are not valid; repair them.",
        "objetos no son válidos; repáralos.",
    ),
    (
        "objects do not sit on the floor.",
        "objetos no están apoyados en el suelo.",
    ),
    ("Printer selection", "Selección de impresora"),
    ("Select a printer", "Seleccionar una impresora"),
    (
        "Online 3D print service",
        "Servicio de impresión 3D en línea",
    ),
    ("Installed printers", "Impresoras instaladas"),
    ("None", "Ninguna"),
    ("Detected printers", "Impresoras detectadas"),
    ("Refresh", "Actualizar"),
    ("Material type", "Tipo de material"),
    ("Select finish and color", "Seleccionar acabado y color"),
    ("Reset orientation", "Rest. orientación"),
    ("Reset scale", "Restabl. escala"),
    ("Objects", "Objetos"),
    ("Triangles", "Triángulos"),
    ("Volume", "Volumen"),
    (
        "There are no current 3D print jobs.",
        "No hay trabajos de impresión 3D actuales.",
    ),
    ("warning", "advertencia"),
    ("warnings", "advertencias"),
    ("Micrometer", "Micrómetro"),
    ("Millimeter", "Milímetro"),
    ("Centimeter", "Centímetro"),
    ("Inch", "Pulgada"),
    ("Foot", "Pie"),
    ("Meter", "Metro"),
    ("Add", "Agregar"),
    ("Shadows", "Sombras"),
    ("Reflections", "Reflejos"),
    ("Introduction", "Introducción"),
    ("Supported printers", "Impresoras compatibles"),
    ("User guide", "Guía del usuario"),
    ("Reduce faces", "Reducir caras"),
    ("Undo", "Deshacer"),
    ("Redo", "Rehacer"),
    ("On", "Activado"),
    ("Off", "Desactivado"),
    ("Selected", "Seleccionado"),
    ("Order a print online", "Pedir impresión en línea"),
    ("Print on paper", "Imprimir en papel"),
    ("About the model", "Acerca del modelo"),
    ("Feedback", "Comentarios"),
    ("Rate this app", "Calificar esta aplicación"),
    ("Special selection", "Selección especial"),
    (
        "One or more objects are not valid. Click here to repair them.",
        "Uno o varios objetos no tienen una definición válida. Haz clic aquí para repararlos.",
    ),
    (
        "To paint, pick a colour in Selected and click the object. Coverage sets how much of the surface around the click is painted.",
        "Para pintar, elige un color en Seleccionado y haz clic en el objeto. Cobertura indica cuánta superficie alrededor del clic se pinta.",
    ),
    ("Object", "Objeto"),
    ("Edit", "Editar"),
    ("Paint", "Pintar"),
    ("View", "Ver"),
    ("Help", "Ayuda"),
    ("3D print", "Impresión 3D"),
    ("Undo (Ctrl+Z)", "Deshacer (Ctrl+Z)"),
    ("Redo (Ctrl+Y)", "Rehacer (Ctrl+Y)"),
    ("Save (Ctrl+S)", "Guardar (Ctrl+S)"),
    ("Coming soon", "Próximamente"),
    // Insert
    ("Custom", "Personalizado"),
    ("Cube", "Cubo"),
    ("Cylinder", "Cilindro"),
    ("Pyramid", "Pirámide"),
    ("Cone", "Cono"),
    ("Sphere", "Esfera"),
    ("Hexagon", "Hexágono"),
    ("Wedge", "Cuña"),
    ("Torus", "Toro"),
    ("Tetrahedron", "Tetraedro"),
    // Object
    ("Duplicate", "Duplicar"),
    ("Copy", "Copiar"),
    ("Cut", "Cortar"),
    ("Paste", "Pegar"),
    ("Delete", "Eliminar"),
    ("Settle", "Instalar"),
    ("Mirror", "Reflejo"),
    ("X axis", "Eje X"),
    ("Y axis", "Eje Y"),
    ("Z axis", "Eje Z"),
    ("Measure", "Medida"),
    ("Distance", "Distancia"),
    ("Snap", "Ajustar"),
    // Edit
    ("Simplify", "Simplificar"),
    ("Split", "Dividir"),
    ("Smooth", "Suave"),
    ("Emboss", "Relieve"),
    ("Extrude down", "Extrudir hacia abajo"),
    ("Merge", "Combinar"),
    ("Intersect", "Intersecar"),
    ("Subtract", "Restar"),
    ("Hollow", "Ahuecar"),
    ("Text", "Texto"),
    ("Size", "Tamaño"),
    ("Depth", "Profundidad"),
    ("Angle", "Ángulo"),
    ("Offset", "Desplazamiento"),
    ("Engrave", "Grabar"),
    ("Pattern", "Patrón"),
    ("Star", "Estrella"),
    ("Heart", "Corazón"),
    ("Circle", "Círculo"),
    ("Image", "Imagen"),
    ("Font", "Fuente"),
    ("Built-in", "Integrada"),
    ("Projection", "Proyección"),
    ("Planar", "Planar"),
    ("Cylindrical", "Cilíndrica"),
    ("Spherical", "Esférica"),
    ("Bevel", "Bisel"),
    ("Top", "Superior"),
    ("Sides", "Lateral"),
    ("Object color", "Color del objeto"),
    (
        "Drag on the object to move; Shift+drag to rotate",
        "Arrastra sobre el objeto para mover; Mayús+arrastrar para girar",
    ),
    ("Minimum wall thickness", "Grosor de pared mínimo"),
    ("Select two or more objects", "Selecciona dos o más objetos"),
    ("Reduction level", "Nivel de reducción"),
    ("Number of faces", "Número de caras"),
    ("Original number of faces", "Número de caras originales"),
    ("Keep top", "Mantener parte superior"),
    ("Keep bottom", "Mantener parte inferior"),
    ("Keep both", "Mantener ambos"),
    ("Height", "Altura"),
    // Paint (the names in `scene::PALETTE`)
    ("Turquoise", "Turquesa"),
    ("Yellow", "Amarillo"),
    ("Green", "Verde"),
    ("Red", "Rojo"),
    ("Orange", "Naranja"),
    ("Purple", "Morado"),
    ("Pink", "Rosa"),
    ("White", "Blanco"),
    ("Gray", "Gris"),
    ("Black", "Negro"),
    ("Color", "Color"),
    ("Material", "Material"),
    ("Texture", "Textura"),
    // View
    ("Center view", "Vista central"),
    ("Shading", "Sombreado"),
    ("Colors", "Colores"),
    ("Smoothing", "Suavizando"),
    ("Wireframe", "Estructura metálica"),
    ("Grid", "Cuadrícula"),
    ("X-ray", "Rayos X"),
    // Help and about
    ("About", "Acerca de"),
    ("About 3D BuildCraft", "Acerca de 3D BuildCraft"),
    ("Version", "Versión"),
    (
        "Build, repair and prepare 3D models for printing.",
        "Crea, repara y prepara modelos 3D para imprimir.",
    ),
    ("Windows, Linux and macOS.", "Windows, Linux y macOS."),
    // Menu and files
    ("New scene", "Nueva escena"),
    ("Open", "Abrir"),
    ("Save", "Guardar"),
    ("Save as", "Guardar como"),
    ("3D printer status", "Estado de la impresora 3D"),
    ("Recent", "Recientes"),
    ("Browse files…", "Buscar archivos…"),
    ("Settings", "Configuración"),
    ("3D models", "Modelos 3D"),
    (
        "3D Manufacturing Format (3MF)",
        "Formato 3D Manufacturing (3MF)",
    ),
    ("Could not open the file:", "No se pudo abrir el archivo:"),
    ("Could not save:", "No se pudo guardar:"),
    ("Saved:", "Guardado:"),
    // Selection pane
    ("Selection", "Selección"),
    ("Group", "Grupo"),
    ("Ungroup", "Desagrupar"),
    ("Select all", "Selección total"),
    ("Select none", "Sin selección"),
    ("Invert selection", "Invertir selección"),
    ("Items", "Elementos"),
    ("triangles", "triángulos"),
    ("vertices", "vértices"),
    ("Open edges", "Bordes abiertos"),
    ("non-manifold", "no-manifold"),
    ("misoriented", "mal orientadas"),
    // Transform bar
    ("Move", "Mover"),
    ("Rotate", "Girar"),
    ("Scale", "Escalar"),
    ("Select one object", "Selecciona un objeto"),
    ("Uniform scale", "Escala uniforme"),
    ("ft", "pie"),
    // Viewport and dialogs
    ("Import model", "Importar modelo"),
    ("File units", "Unidades del archivo"),
    ("Cancel", "Cancelar"),
    ("Capture", "Capturar"),
    ("Close", "Cerrar"),
    ("AI assistant", "Asistente de IA"),
    ("No model chosen", "Ningún modelo elegido"),
    ("New chat", "Nuevo chat"),
    ("Start over; the scene stays as it is.", "Empezar de nuevo; la escena queda como está."),
    ("Ask for changes to the scene, for example: “add a 30 mm cube and engrave my name on top”. The assistant uses the app's tools; each change shows as it is made and can be undone.", "Pide cambios en la escena, por ejemplo: “agrega un cubo de 30 mm y graba mi nombre arriba”. El asistente usa las herramientas de la app; cada cambio se ve mientras se hace y se puede deshacer."),
    ("Nothing is sent anywhere except the agent or endpoint you chose.", "No se envía nada a ningún lado salvo al agente o servicio que elegiste."),
    ("Working…", "Trabajando…"),
    ("Ask the assistant to change the scene…", "Pídele al asistente que cambie la escena…"),
    ("Send", "Enviar"),
    ("Preset", "Predefinido"),
    ("Base URL", "URL base"),
    ("Detected on this computer", "Detectado en este equipo"),
    ("running", "en ejecución"),
    ("models", "modelos"),
    ("Use", "Usar"),
    ("installed", "instalado"),
    ("Connect", "Conectar"),
    ("Show command", "Mostrar comando"),
    ("Manage removal in the agent's settings.", "Gestiona la eliminación en la configuración del agente."),
    ("MCP server connected.", "Servidor MCP conectado."),
    ("Add this MCP server in Antigravity's settings", "Agrega este servidor MCP en la configuración de Antigravity"),
    ("Use model endpoints for the in-app chat, or connect detected agents through MCP to work on the scene. Each change shows on screen and can be undone.", "Usa endpoints de modelos para el chat de la aplicación o conecta los agentes detectados mediante MCP para trabajar en la escena. Cada cambio se muestra en pantalla y se puede deshacer."),
    ("Model", "Modelo"),
    ("Request timeout", "Tiempo de espera"),
    ("API key", "Clave de API"),
    ("optional", "opcional"),
    ("Save key", "Guardar clave"),
    ("Key saved in the system keychain.", "Clave guardada en el llavero del sistema."),
    ("The key is stored in the system keychain, never in the app's settings. Leave it empty for a local server; saving an empty key removes it.", "La clave se guarda en el llavero del sistema, nunca en la configuración de la app. Déjala vacía para un servidor local; guardar una clave vacía la borra."),
    ("Test", "Probar"),
    ("Open the chat", "Abrir el chat"),
    ("External agents (MCP)", "Agentes externos (MCP)"),
    ("To use the tools from an agent outside the app (Claude Code, Claude Desktop, Codex, Cursor or any MCP client), add this command to it as an MCP server:", "Para usar las herramientas desde un agente fuera de la app (Claude Code, Claude Desktop, Codex, Cursor o cualquier cliente MCP), agrégale este comando como servidor MCP:"),
    ("With Claude Code:", "Con Claude Code:"),
    ("With this switch on and the app open, the agent works in this window; otherwise the command opens a new one.", "Con este interruptor activado y la app abierta, el agente trabaja en esta ventana; si no, el comando abre una nueva."),
    ("AI assistant:", "Asistente de IA:"),
    ("Print", "Imprimir"),
    ("Choose a slicer first.", "Elige primero un laminador."),
    (
        "Choose the slicer that prints the model",
        "Elige el laminador que imprime el modelo",
    ),
    ("Installed slicers", "Laminadores instalados"),
    ("Browse…", "Examinar…"),
    ("Opened in", "Abierto en"),
    ("Could not print:", "No se pudo imprimir:"),
    (
        "No camera was found, or it is in use by another app.",
        "No se encontró una cámara, o la está usando otra aplicación.",
    ),
    ("Starting the camera…", "Iniciando la cámara…"),
    ("Unsaved changes will be lost.", "Se perderán los cambios sin guardar."),
    ("Discard scene", "Descartar escena"),
    ("Save scene", "Guardar escena"),
    (
        "🔧 One or more objects are not valid (open or non-manifold mesh).",
        "🔧 Uno o varios objetos no tienen una definición válida (malla abierta o no-manifold).",
    ),
    ("Repair", "Reparar"),
    ("Objects repaired:", "Objetos reparados:"),
    ("Roll", "Rodillo"),
    (
        "Unsaved changes from the previous session were found. Do you want to load them again?",
        "Se detectaron cambios sin guardar de la sesión anterior. ¿Quieres volver a cargarlos?",
    ),
    ("Continue", "Continuar"),
    (
        "Chat with a model endpoint that works on the scene with the app's tools. Each change shows on screen and can be undone. Agents such as Claude Code or Codex attach from outside, through External agents (MCP) below.",
        "Chatea con un servicio de modelos que trabaja en la escena con las herramientas de la app. Cada cambio se ve en pantalla y se puede deshacer. Agentes como Claude Code o Codex se conectan desde afuera, con Agentes externos (MCP) más abajo.",
    ),
    ("Eyedropper", "Gotero"),
    ("More colors...", "Más colores..."),
    ("Recent colors", "Colores recientes"),
    ("Standard colors", "Colores estándares"),
    ("Add to custom colors", "Agregar a colores personalizados"),
    ("Hue", "Matiz"),
    ("Saturation", "Saturación"),
    ("Brightness", "Brillo"),
    ("Accept", "Aceptar"),
    ("More information", "Más información"),
    ("Ignore", "Ignorar"),
    ("Grid properties", "Propiedades de cuadrícula"),
    ("Grid mode", "Modo de cuadrícula"),
    ("Grid None", "Cuadrícula Ninguno"),
    ("Default", "Predeterminado"),
    ("Show grid labels", "Mostrar etiquetas de cuadrícula"),
    ("Repairing...", "Reparando..."),
    ("Loading...", "Cargando..."),
    ("The operation failed.", "La operación falló."),
    ("Pitch", "Tono"),
    ("Yaw", "Yaw"),
    ("deg.", "grad."),
    (
        "The selected object does not overlap any other.",
        "El objeto seleccionado no se superpone con ningún otro.",
    ),
    // Settings
    ("Language", "Idioma"),
    ("Units", "Unidades"),
    ("App color mode", "Modo de color de la aplicación"),
    ("Use system setting", "Usar configuración del sistema"),
    ("Light", "Claro"),
    ("Dark", "Oscuro"),
    ("Collision", "Colisión"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ui_text_has_a_spanish_translation() {
        // Literal texts are written as `tr(lang, "…")` in the UI code, src/app/*.rs.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/app");
        let src: String = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        // rustfmt may break a call over lines, so drop whitespace after `tr(`.
        let mut texts = Vec::new();
        for part in src.split("tr(").skip(1) {
            let rest = part.trim_start();
            let Some(rest) = rest.strip_prefix("lang,") else {
                continue;
            };
            let Some(rest) = rest.trim_start().strip_prefix('"') else {
                continue;
            };
            texts.push(&rest[..rest.find('"').unwrap()]);
        }
        let mut missing: Vec<String> = texts
            .iter()
            .filter(|t| !spanish().contains_key(*t))
            .map(|t| t.to_string())
            .collect();
        // Texts that reach `tr` through a variable or a label() method.
        let mut dynamic: Vec<&str> = crate::primitives::Primitive::ALL
            .iter()
            .map(|p| p.label())
            .collect();
        dynamic.extend(crate::io::Format::SAVE.iter().map(|f| f.label()));
        dynamic.extend(crate::scene::PALETTE.iter().map(|(name, _)| *name));
        dynamic.extend(crate::emboss::Shape::ALL.iter().map(|s| s.label()));
        dynamic.extend(crate::emboss::Projection::ALL.iter().map(|p| p.label()));
        dynamic.extend(crate::settings::Units::ALL.iter().map(|u| u.name()));
        dynamic.extend(crate::image3d::Mode::ALL.iter().map(|m| m.label()));
        dynamic.extend(["Coming soon", "New scene", "ft", "warning", "warnings"]);
        // Names that read the same in both languages.
        let neutral = ["STL", "OBJ", "PLY", "glTF", "GLB"];
        for text in dynamic {
            if !spanish().contains_key(text) && !neutral.contains(&text) {
                missing.push(text.to_string());
            }
        }
        assert!(missing.is_empty(), "missing Spanish for: {missing:?}");
        assert!(
            texts.len() > 100,
            "the scan found only {} texts",
            texts.len()
        );
    }

    #[test]
    fn table_has_no_duplicate_keys() {
        assert_eq!(spanish().len(), ES.len());
    }

    #[test]
    fn locale_environment_precedence() {
        for (values, expected) in [
            ([Some("es_UY"), Some("en_US"), Some("en_US")], Some("es_UY")),
            ([Some(""), Some("es_ES"), Some("en_US")], Some("es_ES")),
            ([None, Some(""), Some("en_US")], Some("en_US")),
            ([None, None, None], None),
        ] {
            let mut values = values.into_iter();
            assert_eq!(
                environment_locale(|_| values.next().flatten().map(str::to_owned)).as_deref(),
                expected
            );
        }
        assert_eq!(Lang::default(), Lang::En);
    }

    #[test]
    fn locale_detection() {
        assert_eq!(Lang::from_locale("es-AR"), Lang::Es);
        assert_eq!(Lang::from_locale("ES"), Lang::Es);
        assert_eq!(Lang::from_locale("en-US"), Lang::En);
        assert_eq!(Lang::from_locale(""), Lang::En);
        assert_eq!(tr(Lang::En, "Save"), "Save");
        assert_eq!(tr(Lang::Es, "Save"), "Guardar");
    }
}
