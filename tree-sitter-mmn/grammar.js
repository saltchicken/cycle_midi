module.exports = grammar({
  name: 'mmn',

  extras: $ => [
    /\s/,
    $.comment,
  ],

  conflicts: $ => [
  ],

  rules: {
    source_file: $ => repeat($._item),
    comment: $ => token(seq('//', /.*/)),

    _item: $ => choice(
      $.directive,
      $.alias_def,
      $.track_def
    ),

    directive: $ => choice(
      seq('#BPM=', $._number),
      seq('#SIG=', $.int, '/', $.int),
      seq('#QUANTIZE=', choice('auto', 'AUTO', $.int)),
      seq('#SCALE=', $.scale_def),
      seq(choice('#SCALE_SEQ=', '#SCALE_SEQ'), $.scale_seq_def),
      '#SILENCE',
      seq('#INCLUDE', $.string)
    ),

    scale_def: $ => seq(choice($.pitch_val, $.int), $.identifier),
    
    scale_seq_def: $ => choice(
      seq('{', sepBy('|', seq('(', $.int, ',', $.int, ')', ':', $.scale_def)), '}'),
      seq('shift', '(', $.scale_def, ',', $.int, optional(seq(',', $.int)), ')'),
      seq('circle_of_fifths', '(', $.scale_def, optional(seq(',', $.int)), ')')
    ),

    alias_def: $ => seq(
        'let', 
        $.identifier, 
        optional(seq('(', sepBy(',', $.identifier), ')')), 
        '=', 
        optional($._parallel_macro)
    ),

    alias_ref: $ => seq(
        $.identifier, 
        optional(seq('(', sepBy(',', $.expr), ')'))
    ),

    track_def: $ => seq(
      optional('!'),
      'T', $.int,
      optional(seq('(', sepBy(',', $.track_modifier), ')')),
      optional(seq('with', repeat1($.postfix))),
      ':',
      optional($._parallel_macro)
    ),

    track_modifier: $ => choice(
      seq('span', ':', $.int),
      seq('speed', ':', $._number),
      seq('scale', ':', $.scale_def),
      seq('pc', ':', $.int),
      seq('octave', ':', $.int),
      seq('seed', ':', $.int, optional(seq(choice('m_every', 't_every', 'every'), $.int)))
    ),

    _parallel_macro: $ => sepBy1('|', repeat1($.expr)),
    _parallel_seq: $ => sepBy1('|', repeat1($.expr)),

    expr: $ => seq(
      $._atom,
      repeat($.postfix)
    ),

    _atom: $ => choice(
      '.', // rest
      '_', // hold
      seq('scale', '(', $.scale_def, ')', $.expr),
      $.subdivision_group,
      $.cycle_block,
      $.seq_group,
      $.alt_group,
      $.rnd_group,
      $.poly_group,
      $.shuf_group,
      $.struct_group,
      $.midi_import,
      $.cc_val,
      $.chord_or_note,
      $.alias_ref
    ),

    subdivision_group: $ => seq('(', optional($._parallel_seq), ')'),
    cycle_block: $ => seq('[', optional($._parallel_seq), ']', optional(seq('*', $.int))),
    seq_group: $ => seq('seq', '(', optional($._parallel_seq), ')'),
    alt_group: $ => seq('alt', '(', repeat($.expr), ')'),
    
    rnd_branch: $ => seq(optional(seq($.int, ':')), optional($._parallel_seq)),
    rnd_group: $ => seq('rnd', '(', sepBy(',', $.rnd_branch), ')'),
    
    poly_group: $ => seq('poly', '(', sepBy(',', repeat1($.expr)), ')'),
    shuf_group: $ => seq('shuf', '(', repeat($.expr), ')'),
    struct_group: $ => seq('struct', '(', $.expr, ',', $.expr, ')'),
    
    kwarg: $ => seq($.identifier, '=', choice($.string, $._number, 'true', 'false')),
    
    midi_import: $ => choice(
      seq('midi', '(', $.string, optional(seq(',', sepBy(',', $.kwarg))), ')'),
      /[a-zA-Z0-9_\/-]+\.midi?/
    ),

    cc_val: $ => seq(choice('cc', 'CC'), $.int, optional(seq('@', $.dynamic_val))),
    
    pitch_val: $ => choice(
        /[A-G][#b]?[-0-9]+/,
        /-?[0-9]+[#b]*/
    ),

    numeric_named_chord: $ => seq(
        $.pitch_val, "'", /[a-zA-Z0-9]+/
    ),

    _pitch_group: $ => prec.left(seq(
      choice($.numeric_named_chord, $.pitch_val),
      optional(seq('@', $.int)),
      optional(seq('%', $.int)) 
    )),

    chord_or_note: $ => prec.left(seq(
      sepBy1('+', $._pitch_group),
      optional(seq('@', $.int)),
      optional(seq('%', $.int)) 
    )),

    _kwarg_label: $ => seq($.identifier, ':'),

    postfix: $ => choice(
      seq('.', choice(
        seq(choice('euclid', 'E'), '(', optional($._kwarg_label), $.int, ',', optional($._kwarg_label), $.int, ')'),
        seq('arp', '(', optional($._kwarg_label), $.identifier, ')'),
        seq('stut', '(', optional($._kwarg_label), $.int, ',', optional($._kwarg_label), $._number, ',', optional($._kwarg_label), $._number, ')'),
        seq('drop', '(', optional($._kwarg_label), $.int, ')'),
        seq('shift', '(', optional($._kwarg_label), $._number, ')'),
        seq('speed', '(', optional($._kwarg_label), $._number, ')'),
        seq('humanize', '(', optional(seq(optional($._kwarg_label), $.int, optional(seq(',', optional($._kwarg_label), $._number, optional('ms'))))), ')'),
        seq('off', '(', optional($._kwarg_label), $._number, ',', repeat1($.postfix), ')'),
        seq('strum', '(', optional($._kwarg_label), $._number, ')'),
        seq('extract', '(', optional($._kwarg_label), $.identifier, optional(seq(',', optional($._kwarg_label), $.int, optional(seq(',', optional($._kwarg_label), $.int)))), ')'),
        seq('chordify', optional(seq('(', optional($._kwarg_label), $.int, optional(seq(',', optional($._kwarg_label), $.int)), ')'))),
        seq('wrap', optional(seq('(', ')'))),
        seq(choice('vel', 'v'), '(', optional($._kwarg_label), $.int, ')'),
        seq(choice('gate', 'g'), '(', optional($._kwarg_label), $.int, ')')
      )),
      // Symbolic Postfixes
      seq('?', $.int),
      seq('^', $.int),
      seq('/', $.int),
      seq('+', $.int), 
      seq('-', $.int),
      seq('*', $.int)
    ),

    identifier: $ => /[a-zA-Z_][a-zA-Z0-9_]*/,
    int: $ => /-?[0-9]+/,
    float: $ => /-?[0-9]+\.[0-9]+/,
    _number: $ => choice($.int, $.float),
    string: $ => /"[^"]*"/,
    dynamic_val: $ => choice(
      seq(choice('sine', 'saw', 'tri'), optional(seq('(', $.int, ',', $.int, optional(seq(',', $._number)), ')'))),
      $.int
    )
  }
});

function sepBy1(sep, rule) {
  return seq(rule, repeat(seq(sep, rule)));
}

function sepBy(sep, rule) {
  return optional(sepBy1(sep, rule));
}
