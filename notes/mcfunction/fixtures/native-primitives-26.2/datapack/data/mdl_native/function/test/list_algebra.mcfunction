data remove storage mdl:observations list_algebra
scoreboard objectives remove mdl_list
scoreboard objectives add mdl_list dummy

# Batch insertion preserves source order. Insertion indices differ from access indices:
# insert -1 means after the final element, while path [-1] selects the final element.
data modify storage mdl:observations list_algebra.batch set value {source:[1,2,3],items:[7,8]}
data modify storage mdl:observations list_algebra.batch.append set from storage mdl:observations list_algebra.batch.source
data modify storage mdl:observations list_algebra.batch.append append from storage mdl:observations list_algebra.batch.items[]
data modify storage mdl:observations list_algebra.batch.prepend set from storage mdl:observations list_algebra.batch.source
data modify storage mdl:observations list_algebra.batch.prepend prepend from storage mdl:observations list_algebra.batch.items[]
data modify storage mdl:observations list_algebra.batch.insert_middle set from storage mdl:observations list_algebra.batch.source
data modify storage mdl:observations list_algebra.batch.insert_middle insert 1 from storage mdl:observations list_algebra.batch.items[]
data modify storage mdl:observations list_algebra.batch.insert_minus_one set from storage mdl:observations list_algebra.batch.source
data modify storage mdl:observations list_algebra.batch.insert_minus_one insert -1 value 9
data modify storage mdl:observations list_algebra.batch.insert_minus_two set from storage mdl:observations list_algebra.batch.source
data modify storage mdl:observations list_algebra.batch.insert_minus_two insert -2 value 9

# Bounds and empty-source behavior, with both command success and result retained.
data modify storage mdl:observations list_algebra.bounds set value {empty:[],source:[1,2,3],no_items:[]}
execute store success storage mdl:observations list_algebra.bounds.insert_at_size.success byte 1 store result storage mdl:observations list_algebra.bounds.insert_at_size.result int 1 run data modify storage mdl:observations list_algebra.bounds.source insert 3 value 4
execute store success storage mdl:observations list_algebra.bounds.insert_too_large.success byte 1 store result storage mdl:observations list_algebra.bounds.insert_too_large.result int 1 run data modify storage mdl:observations list_algebra.bounds.source insert 5 value 5
execute store success storage mdl:observations list_algebra.bounds.insert_negative_front.success byte 1 store result storage mdl:observations list_algebra.bounds.insert_negative_front.result int 1 run data modify storage mdl:observations list_algebra.bounds.source insert -5 value 0
execute store success storage mdl:observations list_algebra.bounds.append_empty_selection.success byte 1 store result storage mdl:observations list_algebra.bounds.append_empty_selection.result int 1 run data modify storage mdl:observations list_algebra.bounds.source append from storage mdl:observations list_algebra.bounds.no_items[]
execute store success storage mdl:observations list_algebra.bounds.remove_empty_wildcard.success byte 1 store result storage mdl:observations list_algebra.bounds.remove_empty_wildcard.result int 1 run data remove storage mdl:observations list_algebra.bounds.empty[]

# "set from" takes the last selected source. Wildcard set broadcasts one copied value.
# On an empty list, wildcard set creates one element instead of doing nothing.
data modify storage mdl:observations list_algebra.set_selection set value {sources:[11,22,33],target:0,filled:[1,2,3],empty:[]}
data modify storage mdl:observations list_algebra.set_selection.target set from storage mdl:observations list_algebra.set_selection.sources[]
data modify storage mdl:observations list_algebra.set_selection.filled[] set value 9
data modify storage mdl:observations list_algebra.set_selection.empty[] set value 9

# Copies are deep: changing a nested value in the source does not mutate the copy.
data modify storage mdl:observations list_algebra.copy set value {source:[{nested:{x:1}}]}
data modify storage mdl:observations list_algebra.copy.clone set from storage mdl:observations list_algebra.copy.source
data modify storage mdl:observations list_algebra.copy.source[0].nested.x set value 2

# Self-insertion reads a copied source selection before mutating the target.
data modify storage mdl:observations list_algebra.self set value {append_case:[1,2,3],prepend_case:[1,2,3],insert_case:[1,2,3]}
data modify storage mdl:observations list_algebra.self.append_case append from storage mdl:observations list_algebra.self.append_case[]
data modify storage mdl:observations list_algebra.self.prepend_case prepend from storage mdl:observations list_algebra.self.prepend_case[]
data modify storage mdl:observations list_algebra.self.insert_case insert 1 from storage mdl:observations list_algebra.self.insert_case[]

# Root compound/list matching is containment, not list equality. Expected list elements
# may appear in any order, and duplicate expected elements may reuse one actual match.
data modify storage mdl:list_match value set value {xs:[1,2,3],records:[{id:"a",extra:1},{id:"b"}]}
execute store success storage mdl:observations list_algebra.match.scalar_contains byte 1 run execute if data storage mdl:list_match {value:{xs:[2]}}
execute store success storage mdl:observations list_algebra.match.unordered_contains byte 1 run execute if data storage mdl:list_match {value:{xs:[3,1]}}
execute store success storage mdl:observations list_algebra.match.duplicate_reuses_match byte 1 run execute if data storage mdl:list_match {value:{xs:[2,2]}}
execute store success storage mdl:observations list_algebra.match.empty_requires_empty byte 1 run execute if data storage mdl:list_match {value:{xs:[]}}
execute store result storage mdl:observations list_algebra.match.compound_filter_count int 1 run execute if data storage mdl:list_match value.records[{id:"a"}]

# Bookshelf and stdmodulesystem exploit the return value of a no-op set as an
# exact runtime equality oracle. Copy the candidate before probing because an
# unequal comparison overwrites the probe target.
data modify storage mdl:observations list_algebra.equality set value {needle:{n:1,x:[2,"three"]},equal:{n:1,x:[2,"three"]},different:{n:1,x:[2,"four"]},numeric_type:1b}
data modify storage mdl:observations list_algebra.equality.equal_probe set from storage mdl:observations list_algebra.equality.equal
execute store success storage mdl:observations list_algebra.equality.equal_set_success byte 1 store result storage mdl:observations list_algebra.equality.equal_set_result int 1 run data modify storage mdl:observations list_algebra.equality.equal_probe set from storage mdl:observations list_algebra.equality.needle
data modify storage mdl:observations list_algebra.equality.different_probe set from storage mdl:observations list_algebra.equality.different
execute store success storage mdl:observations list_algebra.equality.different_set_success byte 1 store result storage mdl:observations list_algebra.equality.different_set_result int 1 run data modify storage mdl:observations list_algebra.equality.different_probe set from storage mdl:observations list_algebra.equality.needle
data modify storage mdl:observations list_algebra.equality.numeric_probe set value 1
execute store success storage mdl:observations list_algebra.equality.numeric_type_set_success byte 1 store result storage mdl:observations list_algebra.equality.numeric_type_set_result int 1 run data modify storage mdl:observations list_algebra.equality.numeric_probe set from storage mdl:observations list_algebra.equality.numeric_type

# Counting exact values in one native list pass: set every element of a copy to
# the needle. The result counts changed elements, so equal_count = length - result.
data modify storage mdl:observations list_algebra.equality.count_source set value [1,2,1,3,1]
data modify storage mdl:observations list_algebra.equality.count_probe set from storage mdl:observations list_algebra.equality.count_source
data modify storage mdl:observations list_algebra.equality.count_needle set value 1
execute store result score #equality_length mdl_list run data get storage mdl:observations list_algebra.equality.count_probe
execute store result score #equality_changed mdl_list run data modify storage mdl:observations list_algebra.equality.count_probe[] set from storage mdl:observations list_algebra.equality.count_needle
scoreboard players operation #equality_length mdl_list -= #equality_changed mdl_list
execute store result storage mdl:observations list_algebra.equality.exact_count int 1 run scoreboard players get #equality_length mdl_list
data modify storage mdl:observations list_algebra.equality.empty_count_probe set value []
execute store result score #empty_length mdl_list run data get storage mdl:observations list_algebra.equality.empty_count_probe
execute store result score #empty_changed mdl_list run data modify storage mdl:observations list_algebra.equality.empty_count_probe[] set from storage mdl:observations list_algebra.equality.count_needle
scoreboard players operation #empty_length mdl_list -= #empty_changed mdl_list
execute store result storage mdl:observations list_algebra.equality.unguarded_empty_count int 1 run scoreboard players get #empty_length mdl_list

# Filtered paths update/remove every matching compound and return the changed count.
data modify storage mdl:observations list_algebra.filtered set value [{kind:"x",n:1},{kind:"y",n:2},{kind:"x",n:3}]
execute store result storage mdl:observations list_algebra.filtered_set_count int 1 run data modify storage mdl:observations list_algebra.filtered[{kind:"x"}].n set value 0
execute store result storage mdl:observations list_algebra.filtered_remove_count int 1 run data remove storage mdl:observations list_algebra.filtered[{kind:"x"}]

# Array tags remain homogeneous even though ordinary lists are heterogeneous.
data modify storage mdl:observations list_algebra.array set value [I;1,2,3]
execute store success storage mdl:observations list_algebra.array_wrong_type.success byte 1 store result storage mdl:observations list_algebra.array_wrong_type.result int 1 run data modify storage mdl:observations list_algebra.array append value "x"

# Composition: stack push/pop at the tail, naive queue dequeue at the head, and
# a reverse built only from copy + peek-last + append + remove-last.
data modify storage mdl:observations list_algebra.stack set value [1,2]
data modify storage mdl:observations list_algebra.stack append value 3
data modify storage mdl:observations list_algebra.stack_popped set from storage mdl:observations list_algebra.stack[-1]
data remove storage mdl:observations list_algebra.stack[-1]
data modify storage mdl:observations list_algebra.queue set value [1,2,3]
data modify storage mdl:observations list_algebra.queue_dequeued set from storage mdl:observations list_algebra.queue[0]
data remove storage mdl:observations list_algebra.queue[0]
data modify storage mdl:queue in set value []
data modify storage mdl:queue out set value []
data modify storage mdl:queue input set value 1
function mdl_native:queue/push
data modify storage mdl:queue input set value 2
function mdl_native:queue/push
data modify storage mdl:queue input set value 3
function mdl_native:queue/push
function mdl_native:queue/pop
data modify storage mdl:observations list_algebra.two_stack_queue.popped append from storage mdl:queue output
data modify storage mdl:queue input set value 4
function mdl_native:queue/push
function mdl_native:queue/pop
data modify storage mdl:observations list_algebra.two_stack_queue.popped append from storage mdl:queue output
data modify storage mdl:observations list_algebra.two_stack_queue.remaining set from storage mdl:queue
data modify storage mdl:list_work source set value [1,"two",{n:3},[4]]
function mdl_native:list/reverse
data modify storage mdl:observations list_algebra.reversed set from storage mdl:list_work out

execute store result storage mdl:observations list_algebra.final_source_length int 1 run data get storage mdl:observations list_algebra.bounds.source
data get storage mdl:observations list_algebra
