#include "ratex_flutter_linker.h"
#include "ratex.h"

typedef RatexResult (*RatexParseAndLayoutFn)(const char *,
                                             const RatexOptions *);
typedef void (*RatexFreeDisplayListFn)(char *);
typedef const char *(*RatexGetLastErrorFn)(void);

__attribute__((used))
static RatexParseAndLayoutFn ratex_parse_and_layout_anchor =
    ratex_parse_and_layout;

__attribute__((used))
static RatexFreeDisplayListFn ratex_free_display_list_anchor =
    ratex_free_display_list;

__attribute__((used))
static RatexGetLastErrorFn ratex_get_last_error_anchor =
    ratex_get_last_error;

void ratex_flutter_linker_anchor(void) {
  (void)ratex_parse_and_layout_anchor;
  (void)ratex_free_display_list_anchor;
  (void)ratex_get_last_error_anchor;
}
