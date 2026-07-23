scoreboard players operation #ptr_target brainfuck.re += #value brainfuck.re
scoreboard players operation #ptr_target brainfuck.re %= #256 brainfuck.re

#tellraw @a [{text:"value: "},{score:{name:"#value",objective:"brainfuck.re"}}]
#tellraw @a [{text:"target: "},{score:{name:"#ptr_target",objective:"brainfuck.re"}}]

scoreboard players set #sync brainfuck.re 1