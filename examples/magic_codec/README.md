# Magic Codec

Runnable example for the [magic codec](../../docs/guide/magic-codec.md):
`demo.py` declares `# coding: sqlty`, so its `--sql` strings are wrapped
when Python reads the file.

```bash
python examples/magic_codec/demo.py
```

The codec is registered by `sqlty_magic.pth`, which installing sqlty places
in `site-packages`; editable installs of a sqlty checkout don't have it, so
run `python -c "import sqlty.magic, runpy; runpy.run_path('examples/magic_codec/demo.py', run_name='__main__')"`
there. `demo.py` imports `SQL` and `datetime` itself, so running it as a
script works around [#111](https://github.com/jirikuncar/sqlty/issues/111).
