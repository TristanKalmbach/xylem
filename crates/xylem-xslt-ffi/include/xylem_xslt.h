/* Xylem's XSLT engine as a C library. All strings are UTF-8. No panic or
 * exception crosses this boundary: an internal failure is an error return.
 * See docs/embedding.md. MIT licence. */
#ifndef XYLEM_XSLT_H
#define XYLEM_XSLT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Write the result the way a publishing service saves it (no XML
 * declaration, xsl:output ignored). */
#define XYLEM_OUTPUT_PUBLISH   0u
/* Write the result as the stylesheet's xsl:output asks. */
#define XYLEM_OUTPUT_SERIALIZE 1u
/* Refuse document() and external entities in the input. */
#define XYLEM_NO_FILE_ACCESS   2u

typedef struct XylemStylesheet XylemStylesheet;

/* The library version, such as "0.6.0". */
const char *xylem_xslt_version(void);
/* The last error on the calling thread; valid until the next call on it. */
const char *xylem_xslt_last_error(void);
/* What the last transformation on this thread reported besides its result,
 * as JSON {"messages":[...],"warnings":[...]} (xsl:message texts, warnings
 * about recovered errors such as an unreadable document()). */
const char *xylem_xslt_last_log(void);

/* Compiles a stylesheet file and what it includes or imports.
 * Returns NULL on failure. */
XylemStylesheet *xylem_xslt_compile(const char *path);
/* 1 or 2: the XSLT version the stylesheet runs as (0 for NULL). */
uint32_t xylem_xslt_version_of(const XylemStylesheet *ss);

/* Transforms an XML document. params_json is NULL or a JSON object of
 * parameters ({"mode":"final"}). flags combines the XYLEM_* values.
 * Returns 0 and sets *out/*out_len (free with xylem_xslt_free_bytes),
 * or on failure 1 (bad arguments), 2 (the stylesheet or input failed) or
 * 3 (an internal engine failure; the handle stays usable).
 * One handle can run transformations on many threads at once. */
int32_t xylem_xslt_transform(const XylemStylesheet *ss,
                             const uint8_t *input, size_t input_len,
                             const char *params_json, uint32_t flags,
                             uint8_t **out, size_t *out_len);

void xylem_xslt_free_bytes(uint8_t *ptr, size_t len);
void xylem_xslt_free(XylemStylesheet *ss);

#ifdef __cplusplus
}
#endif
#endif
