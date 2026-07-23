execute store result storage brainfuck:re las_ptr int 1 run scoreboard players get #ptr brainfuck.re

scoreboard players operation #ptr brainfuck.re += #value brainfuck.re
scoreboard players operation #ptr brainfuck.re %= #array_length brainfuck.re

function brainfuck:compile/convert/score_to_storage/ptr

function brainfuck:compile/process/shift/_ with storage brainfuck:re