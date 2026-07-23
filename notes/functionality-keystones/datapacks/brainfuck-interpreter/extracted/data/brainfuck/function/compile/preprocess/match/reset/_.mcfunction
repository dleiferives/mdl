scoreboard players remove #diff brainfuck.re 1
execute if score #diff brainfuck.re = #match_pos brainfuck.re run return 1

function brainfuck:compile/convert/score_to_storage/diff
function brainfuck:compile/preprocess/match/reset/get with storage brainfuck:re

execute unless function brainfuck:compile/preprocess/match/reset/detect run return fail

return run function brainfuck:compile/preprocess/match/reset/_