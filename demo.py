from glob import glob
import demoreel

with open("Round_1_Map_1_Borneo.dem", "rb") as istrm:
    demo_data = istrm.read()

inputs = demoreel.unspool(
    demo_data, json_path="$.players[*][?(@.class!='other')]", tick_freq=1
)
pass
