#ir_ip从0开始 code_length从1开始（字符串长度）

#tellraw @a {score:{name:"#ip",objective:"brainfuck.re"}}
execute if score #count brainfuck.re > #max_total_command_count brainfuck.re run return run function brainfuck:error/runtime/too_many_executions
execute if score #cmd_stop brainfuck.re matches 1 run return run function brainfuck:compile/get_char/end
execute if score #ir_ip brainfuck.re >= #code_length brainfuck.re run return run function brainfuck:compile/get_char/end


execute as @e[type=marker,tag=bf.compl,limit=80,distance=..0.1] run function brainfuck:compile/get_char/a


#tellraw @a [{text:"ip: "},{score:{name:"#ir_ip",objective:"brainfuck.re"}}]

execute if score #count_c brainfuck.re <= #max_single_command_count brainfuck.re run return run function brainfuck:compile/get_char/
scoreboard players add #stopwatch brainfuck.re 1
schedule function brainfuck:compile/get_char/ 1t
scoreboard players set #count_c brainfuck.re 0