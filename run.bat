rem .\target\release\ifc_to_3dtiles.exe --input "..\CJ02-金門大橋_F03_20260522high.ifc" --output .\out --normal-mode both --tile-max-triangles 25000 --tile-max-features 50 --overwrite --source-epsg 3825
rem .\target\release\ifc_to_3dtiles.exe --input "..\測試一.rvt" --output .\out --normal-mode both --tile-max-triangles 25000 --tile-max-features 50 --overwrite --source-epsg 3826
rem .\target\release\ifc_to_3dtiles.exe --input "..\測試二(尺寸調整).rvt" --output .\out --normal-mode both --tile-max-triangles 25000 --tile-max-features 50 --overwrite --source-epsg 3826
.\target\release\ifc_to_3dtiles.exe glb-to-3dtiles --input "..\Terrain Remaked.glb" --output .\out --longitude 120.644660 --latitude 24.102594 --height 0 --tile-target-bytes 2500000 --overwrite
