from glob import glob
import demoreel

demo_path = glob("./demoreel/*.dem")[0]
with open(demo_path, "rb") as istrm:
    demo_data = istrm.read()

inputs = demoreel.unspool(demo_data)
pass
