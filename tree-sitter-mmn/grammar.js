module.exports = grammar({
  name: 'mmn',

  extras: $ => [
    /\s/,
    $.comment,
  ],

  conflicts: $ => [
    [$.parallel, $.polymeter],
    [$.dynamic_val],
    [$.postfix],
    [$.expr]
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
      seq('#SCALE_SEQ=', $.scale_seq_def),
      '#SILENCE',
      seq('#INCLUDE', $.string)
    ),

    scale_def: $ => seq(choice($.pitch_val, $.int), $.identifier),
    scale_seq_def: $ => seq(
      '{',
      sepBy('|', seq('(', $.int, ',', $.int, ')', ':', $.scale_def)),
      '}'
    ),

    alias_def: $ => seq('$', $.identifier, '=', $.expr),

    track_def: $ => seq(
      optional('!'),
      'T', 
      $.int,
      repeat($.track_modifier),
      ':',
      $.expr
    ),

    track_modifier: $ => choice(
      seq(choice('fast', 'slow'), $._number),
      seq('scale', $.scale_def),
      seq('pc', $.int),
      seq(choice('up', 'down'), optional($.int)),
      seq('seed', $.int, optional(seq(choice('m_every', 't_every', 'every'), $.int)))
    ),

    expr: $ => seq(
      $._atom,
      repeat($.postfix)
    ),

    _atom: $ => choice(
      '.', // rest
      '_', // hold
      seq('$', $.identifier), // alias ref
      seq('scale', '(', $.scale_def, ')', $.expr),
      $.sequence,
      $.shuffled_sequence,
      $.alternator,
      $.parallel,
      $.polymeter,
      $.seqp,
      $.cc_val,
      $.chord_or_note
    ),

    // Supports optional sequence weights (e.g., "4: 0_t")
    choice_branch: $ => seq(
      optional(seq($.int, ':')),
      repeat1($.expr)
    ),

    sequence: $ => seq('[', sepBy('|', $.choice_branch), ']'),
    shuffled_sequence: $ => seq('shuf', '[', repeat($.expr), ']'),
    alternator: $ => seq('<', repeat($.expr), '>'),
    parallel: $ => seq('{', sepBy('|', repeat($.expr)), '}'),
    polymeter: $ => seq('{', sepBy(',', repeat($.expr)), '}'),
    
    seqp: $ => seq(
      choice('seqP', 'seqPLoop'), 
      '{', 
      sepBy('|', seq('(', $.int, ',', $.int, ')', ':', $.expr)), 
      '}'
    ),

    cc_val: $ => seq(choice('cc', 'CC'), $.int, optional(seq('@', $.dynamic_val))),
    
    // Captures both absolute (C3_maj) and numeric (0_t) chords
    pitch_val: $ => choice(
        /[A-G][#b]?[-0-9]+(?:_[a-zA-Z0-9]+)?/,
        /-?[0-9]+_[a-zA-Z0-9]+/
    ),

    // Notes can be specific pitches, drum aliases, or raw numeric scale degrees (integers)
    chord_or_note: $ => seq(
      sepBy1('+', choice($.pitch_val, $.drum_val, $.int)),
      optional(seq('@', $.int)), // velocity
      optional(seq('%', $.int))  // gate
    ),

    postfix: $ => seq(
      $._postfix_op,
      optional($._cond),
      optional($._m_cond)
    ),

    _postfix_op: $ => choice(
      seq('(', $.int, ',', $.int, ')'), // euclidean
      seq(choice('*', '/'), $._number),   // speed
      seq('arp', '(', $.identifier, ')'),
      seq('ratchet', '(', $.int, ')'),
      seq('stut', '(', $.int, ',', $._number, ',', $._number, ')'),
      seq('humanize', '(', $.int, optional(seq(',', $._number, optional('ms'))), ')'),
      seq(choice('up', 'down'), $.int),
      seq('?', $.int), // probability
      seq(choice('~>', '<~'), $._number),
      seq('shift', '(', $._number, ')'),
      seq('^', $.int), // invert
      seq('drop', '(', $.int, ')'),
      seq('strum', '(', $._number, ')'),
      seq(choice('only', 'm_only'), '(', $.int, optional(seq(',', $.int)), ')'),
      $._cond,
      $._m_cond
    ),

    _cond: $ => seq('if', '(', $.int, optional(seq(',', $.int)), ')'),
    _m_cond: $ => seq('m_if', '(', $.int, optional(seq(',', $.int)), ')'),

    identifier: $ => /[a-zA-Z_][a-zA-Z0-9_]*/,
    int: $ => /-?[0-9]+/,
    float: $ => /-?[0-9]+\.[0-9]+/,
    _number: $ => choice($.int, $.float),
    string: $ => /"[^"]*"/,
    drum_val: $ => choice('bd', 'sn', 'cp', 'lt', 'ch', 'mt', 'oh', 'ht'),
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
