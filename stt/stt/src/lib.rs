wit_bindgen::generate!({
    path: "../wit",
    world: "stt-library",
    generate_all,
    generate_unused_types: true,
    additional_derives: [PartialEq, golem_rust::FromValueAndType, golem_rust::IntoValue],
    pub_export_macro: true,
});

// re-export generated exports
pub use crate::exports::golem;
pub use __export_stt_library_impl as export_stt;
