#include <stdlib.h>
#include <string.h>

typedef struct cmark_parser cmark_parser;
typedef struct cmark_node cmark_node;
typedef struct cmark_llist cmark_llist;

#define CMARK_OPT_SAFE (1 << 3)

cmark_parser *cmark_parser_new(int options);
void cmark_parser_feed(cmark_parser *parser, const char *buffer, size_t len);
cmark_node *cmark_parser_finish(cmark_parser *parser);
void cmark_parser_free(cmark_parser *parser);
void cmark_node_free(cmark_node *node);
char *cmark_render_html(cmark_node *root, int options, cmark_llist *extensions);

typedef struct cmark_syntax_extension cmark_syntax_extension;
void cmark_gfm_core_extensions_ensure_registered(void);
cmark_syntax_extension *cmark_find_syntax_extension(const char *name);
int cmark_parser_attach_syntax_extension(cmark_parser *parser, cmark_syntax_extension *extension);

static char *mdv_rendered_html;

const char *mdv_render_markdown(const char *source) {
  cmark_parser *parser;
  cmark_node *document;
  cmark_syntax_extension *extension;

  cmark_gfm_core_extensions_ensure_registered();
  parser = cmark_parser_new(CMARK_OPT_SAFE);
  extension = cmark_find_syntax_extension("table");
  cmark_parser_attach_syntax_extension(parser, extension);
  extension = cmark_find_syntax_extension("strikethrough");
  cmark_parser_attach_syntax_extension(parser, extension);
  extension = cmark_find_syntax_extension("tasklist");
  cmark_parser_attach_syntax_extension(parser, extension);
  extension = cmark_find_syntax_extension("autolink");
  cmark_parser_attach_syntax_extension(parser, extension);
  cmark_parser_feed(parser, source, strlen(source));
  document = cmark_parser_finish(parser);

  free(mdv_rendered_html);
  mdv_rendered_html = cmark_render_html(document, CMARK_OPT_SAFE, NULL);
  cmark_node_free(document);
  cmark_parser_free(parser);
  return mdv_rendered_html;
}
