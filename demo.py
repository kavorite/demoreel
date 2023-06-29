from glob import glob

import demoreel

with open("demos/Round_1_Map_1_Borneo.dem", "rb") as istrm:
    demo_data = istrm.read()

dtrace = demoreel.dtrace(demo_data)
pass
