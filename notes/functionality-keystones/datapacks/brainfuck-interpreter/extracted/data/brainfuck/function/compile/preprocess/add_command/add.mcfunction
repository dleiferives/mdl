execute if score #add_value brainfuck.re matches 0 run return 1
data modify storage brainfuck:re code.list append value {cmd:"add",value:-1}

scoreboard players add #ir_ip brainfuck.re 1

execute store result storage brainfuck:re code.list[-1].value int 1 run scoreboard players get #add_value brainfuck.re

return 1