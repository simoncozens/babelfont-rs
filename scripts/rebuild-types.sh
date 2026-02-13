 ~/others-repos/typeshare/target/release/typeshare -o babelfont-py/src/babelfont/underlying.py -l python  babelfont/src
 ~/others-repos/typeshare/target/release/typeshare -o babelfont-ts/src/underlying.ts -l typescript babelfont/src
cd babelfont-ts/src/; npx pretter -w . ; cd ../..
