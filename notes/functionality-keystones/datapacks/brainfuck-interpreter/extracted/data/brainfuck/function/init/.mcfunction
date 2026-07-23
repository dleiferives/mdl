#brainfuck:code里存了code（代码），input（输入），array_length（数组长度）
execute if score #running brainfuck.re matches 1 run return run function brainfuck:error/runtime/already_running
scoreboard players set #cmd_stop brainfuck.re 0
scoreboard players set #running brainfuck.re 1

#复制code准备转换成list
data modify storage brainfuck:re code.string set from storage brainfuck:code code
data remove storage brainfuck:re code.list
data modify storage brainfuck:re code.list set value []
#初始化array指针
scoreboard players set #ptr brainfuck.re 0
data modify storage brainfuck:re ptr set value 0
#初始化输出
data remove storage brainfuck:re output.list
data modify storage brainfuck:re output.list set value []
#初始化array_length，默认为30
execute store result score #array_length brainfuck.re run data get storage brainfuck:code array_length
#初始化array
function brainfuck:init/array/
#复制input
data modify storage brainfuck:re input set from storage brainfuck:code input
#初始化jump_table
data remove storage brainfuck:re jump_table
data modify storage brainfuck:re jump_table set value []
#堆栈，用于jump_table
data remove storage brainfuck:re stack
data modify storage brainfuck:re stack set value []



scoreboard players set #code_length brainfuck.re 0


scoreboard players set #ip brainfuck.re 0
#function brainfuck:compile/preprocess/convert/string_to_list/
#execute store success score #tmp_value brainfuck.re run function brainfuck:compile/preprocess/convert/string_to_list/
scoreboard players set #last_char brainfuck.re -100
scoreboard players set #ir_ip brainfuck.re 0

scoreboard players set #add_value brainfuck.re 0
scoreboard players set #addsub brainfuck.re 0
scoreboard players set #shift_value brainfuck.re 0
scoreboard players set #sync brainfuck.re 0
#用于[+]型的优化
scoreboard players set #match_pos brainfuck.re 0
scoreboard players set #ir_ip brainfuck.re 0
execute unless function brainfuck:compile/preprocess/loop/ run return run function brainfuck:error/syntax/close_bracket


execute unless data storage brainfuck:re {stack:[]} run return run function brainfuck:error/syntax/open_bracket
scoreboard players operation #code_length brainfuck.re = #ir_ip brainfuck.re


scoreboard players set #ptr_target brainfuck.re 0

scoreboard players set #count brainfuck.re 0
scoreboard players set #count_c brainfuck.re 0
scoreboard players set #ir_ip brainfuck.re 0

scoreboard players set #stopwatch brainfuck.re 1

schedule function brainfuck:compile/get_char/ 1t