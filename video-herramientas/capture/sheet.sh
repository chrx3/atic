# usage: sheet.sh name  -> sheet-name.png (every 0.5s, 5 cols x 2 rows; tiles = 0.0,0.5,...)
ffmpeg -v error -y -i clips/$1.mp4 -vf "fps=2,scale=432:-1,tile=5x2" -frames:v 1 sheet-$1.png
