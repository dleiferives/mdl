scoreboard players operation #diff brainfuck.re = #ir_ip brainfuck.re

scoreboard players set #ip_offset brainfuck.re 0
execute unless function brainfuck:compile/preprocess/match/reset/_ run return fail

return 1