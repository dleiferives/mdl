# Compound CRUD, merge algebra, dynamic path rendering, structural probes,
# exact snapshot comparison, and typed list-filter arguments on vanilla 26.2.
data remove storage mdl:observations nbt_dict

# Native compound size and recursive merge behavior.
data modify storage mdl:observations nbt_dict.compound set value {a:1,nested:{x:1,y:2},replace_me:[1,2]}
execute store result storage mdl:observations nbt_dict.compound_size int 1 run data get storage mdl:observations nbt_dict.compound
execute store result storage mdl:observations nbt_dict.merge_changed int 1 store success storage mdl:observations nbt_dict.merge_success byte 1 run data modify storage mdl:observations nbt_dict.compound merge value {b:2,nested:{y:9,z:3},replace_me:{now:"compound"}}
execute store result storage mdl:observations nbt_dict.merge_noop_result int 1 store success storage mdl:observations nbt_dict.merge_noop_success byte 1 run data modify storage mdl:observations nbt_dict.compound merge value {b:2,nested:{y:9,z:3},replace_me:{now:"compound"}}
execute store result storage mdl:observations nbt_dict.remove_present_result int 1 store success storage mdl:observations nbt_dict.remove_present_success byte 1 run data remove storage mdl:observations nbt_dict.compound.a
execute store result storage mdl:observations nbt_dict.remove_absent_result int 1 store success storage mdl:observations nbt_dict.remove_absent_success byte 1 run data remove storage mdl:observations nbt_dict.compound.absent

# Missing compound parents are created along a static path. Traversal fails when
# an existing intermediate value has an incompatible type.
execute store success storage mdl:observations nbt_dict.create_child_existing_parent byte 1 run data modify storage mdl:observations nbt_dict.compound.new_child set value 7
execute store success storage mdl:observations nbt_dict.create_child_missing_parent byte 1 run data modify storage mdl:observations nbt_dict.missing_parent.child set value 8
data modify storage mdl:observations nbt_dict.scalar_parent set value 9
execute store success storage mdl:observations nbt_dict.create_child_scalar_parent byte 1 run data modify storage mdl:observations nbt_dict.scalar_parent.child set value 10

# Structurally probe compounds and list selections. Empty-list [] selects no
# elements, so it is not a general empty-list type predicate.
data modify storage mdl:observations nbt_dict.probe_values set value {empty_compound:{},compound:{x:1},empty_list:[],list:[1],bytes:[B;1b],scalar:1,string:"x"}
execute store success storage mdl:observations nbt_dict.probes.empty_compound_as_compound byte 1 run execute if data storage mdl:observations nbt_dict.probe_values.empty_compound{}
execute store success storage mdl:observations nbt_dict.probes.compound_as_compound byte 1 run execute if data storage mdl:observations nbt_dict.probe_values.compound{}
execute store success storage mdl:observations nbt_dict.probes.list_as_compound byte 1 run execute if data storage mdl:observations nbt_dict.probe_values.list{}
execute store success storage mdl:observations nbt_dict.probes.empty_list_elements byte 1 run execute if data storage mdl:observations nbt_dict.probe_values.empty_list[]
execute store success storage mdl:observations nbt_dict.probes.list_elements byte 1 run execute if data storage mdl:observations nbt_dict.probe_values.list[]
execute store success storage mdl:observations nbt_dict.probes.byte_array_elements byte 1 run execute if data storage mdl:observations nbt_dict.probe_values.bytes[]
execute store result storage mdl:observations nbt_dict.probes.empty_list_get_result int 1 store success storage mdl:observations nbt_dict.probes.empty_list_get_success byte 1 run data get storage mdl:observations nbt_dict.probe_values.empty_list

# Compiler-rendered NBT path segments. The macro receives an already encoded
# path segment, not a raw user key.
data modify storage mdl:observations nbt_dict.dynamic set value {}
data modify storage mdl:observations nbt_dict.args set value {plain:{label:"plain",segment:"alpha",value:1},space:{label:"space",segment:'"space key"',value:2},dot:{label:"dot",segment:'"a.b"',value:3},quote:{label:"quote",segment:'"a\\"b"',value:4},backslash:{label:"backslash",segment:'"a\\\\b"',value:5},numeric:{label:"numeric",segment:'"123"',value:6},empty:{label:"empty",segment:'""',value:7}}
function mdl_native:nbt/set_dynamic_key with storage mdl:observations nbt_dict.args.plain
function mdl_native:nbt/set_dynamic_key with storage mdl:observations nbt_dict.args.space
function mdl_native:nbt/set_dynamic_key with storage mdl:observations nbt_dict.args.dot
function mdl_native:nbt/set_dynamic_key with storage mdl:observations nbt_dict.args.quote
function mdl_native:nbt/set_dynamic_key with storage mdl:observations nbt_dict.args.backslash
function mdl_native:nbt/set_dynamic_key with storage mdl:observations nbt_dict.args.numeric
function mdl_native:nbt/set_dynamic_key with storage mdl:observations nbt_dict.args.empty

# A typed int-array macro argument can be embedded directly into a compound
# list filter. No 65,536-entry hexadecimal lookup table is needed for storage
# identity lookup.
data modify storage mdl:observations nbt_dict.uuid_filter set value {records:[{UUID:[I;181,0,0,3],value:"old"},{UUID:[I;182,-1,7,99],value:"other"}],args:{uuid:[I;181,0,0,3],value:"updated"}}
function mdl_native:nbt/set_uuid_record with storage mdl:observations nbt_dict.uuid_filter.args

# Exact equality/change oracle and an observer-style previous-value snapshot.
data modify storage mdl:observations nbt_dict.observer set value {previous:{state:{hp:20,flags:[1b,0b]}},current:{state:{hp:20,flags:[1b,0b]}}}
data modify storage mdl:observations nbt_dict.observer.scratch set from storage mdl:observations nbt_dict.observer.current
execute store result storage mdl:observations nbt_dict.observer.equal_changed_count int 1 store success storage mdl:observations nbt_dict.observer.equal_changed_success byte 1 run data modify storage mdl:observations nbt_dict.observer.scratch set from storage mdl:observations nbt_dict.observer.previous
data modify storage mdl:observations nbt_dict.observer.current.state.hp set value 19
data modify storage mdl:observations nbt_dict.observer.scratch set from storage mdl:observations nbt_dict.observer.current
execute store result storage mdl:observations nbt_dict.observer.different_changed_count int 1 store success storage mdl:observations nbt_dict.observer.different_changed_success byte 1 run data modify storage mdl:observations nbt_dict.observer.scratch set from storage mdl:observations nbt_dict.observer.previous
data modify storage mdl:observations nbt_dict.observer.previous set from storage mdl:observations nbt_dict.observer.current

data get storage mdl:observations nbt_dict
