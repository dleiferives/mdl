use super::TargetSpec;

pub(super) static SPEC: TargetSpec = TargetSpec {
    game_version: "26.2",
    data_pack_format: [107, 1],
    function_directory: "function",
    function_tag_directory: "tags/function",
    default_max_command_sequence: 65_536,
    default_max_command_forks: 65_536,
    max_logical_command_utf16_units: 2_000_000,
    conformance_java_runtime: 25,
};
