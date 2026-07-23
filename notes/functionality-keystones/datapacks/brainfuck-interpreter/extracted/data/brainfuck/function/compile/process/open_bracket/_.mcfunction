execute unless score #ptr_target brainfuck.re matches 0 run return 1
#如果为0，则进行跳转，转到匹配的]后
scoreboard players operation #ir_ip brainfuck.re = #value brainfuck.re
#$execute store result score #ip brainfuck.re run data get storage brainfuck:re jump_table[$(ip)]
#scoreboard players add #ip brainfuck.re 1