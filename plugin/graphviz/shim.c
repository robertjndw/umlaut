/* Thin C layer between umlaut's Rust code and Graphviz's dot layout.
 *
 * Rust hands over DOT text and gets back plain numbers (see umlaut_layout), so no
 * Graphviz struct ever crosses the FFI boundary. Text is never measured by
 * Graphviz: every label in the DOT input is spelled "<width>,<height>" in points and
 * the text layout engine below just reads those numbers back. That way dot reserves
 * exactly the room our Typst-measured labels need.
 */
#include "config.h"

#include <ctype.h>
#include <math.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <cgraph/cgraph.h>
#include <common/render.h>
#include <gvc/gvc.h>
#include <gvc/gvcint.h>
#include <gvc/gvplugin.h>
#include <gvc/gvplugin_textlayout.h>

extern gvplugin_library_t gvplugin_dot_layout_LTX_library;

#ifdef __wasi__
/* wasi-libc's getenv first asks the host for the environment, and its WASI import is
 * stubbed out in the final module (Typst provides none), which would make libc exit.
 * Graphviz only reads optional tuning variables, so there is nothing to find. */
char *getenv(const char *name) {
    (void)name;
    return NULL;
}
#endif

static bool sized_text(textspan_t *span, char **fontpath) {
    double w = 0, h = 0;
    if (sscanf(span->str, "%lf,%lf", &w, &h) != 2) {
        return false;
    }
    span->size.x = w;
    span->size.y = h;
    span->yoffset_layout = 0;
    span->yoffset_centerline = 0.1 * h;
    span->layout = NULL;
    span->free_layout = NULL;
    if (fontpath) {
        *fontpath = NULL;
    }
    return true;
}

static gvtextlayout_engine_t sized_engine = {sized_text};
static gvplugin_installed_t sized_types[] = {{0, "textlayout", 2, &sized_engine, NULL}, {0, NULL, 0, NULL, NULL}};
static gvplugin_api_t sized_apis[] = {{API_textlayout, sized_types}, {(api_t)0, 0}};
static gvplugin_library_t sized_library = {"umlaut", sized_apis};

static const lt_symlist_t builtins[] = {
    {"gvplugin_dot_layout_LTX_library", &gvplugin_dot_layout_LTX_library},
    {"gvplugin_umlaut_LTX_library", &sized_library},
    {NULL, NULL},
};

static char error_text[1024];

/* Graphviz reports a message in pieces ("Error", ": ", the text); keep them all and
 * only look at the result when a call failed. */
static int collect_error(char *message) {
    size_t used = strlen(error_text);
    snprintf(error_text + used, sizeof error_text - used, "%s", message);
    return 0;
}

typedef struct {
    double *data;
    size_t len, cap;
} Buf;

static void push(Buf *b, double v) {
    if (b->len == b->cap) {
        b->cap = b->cap ? b->cap * 2 : 1024;
        b->data = realloc(b->data, b->cap * sizeof(double));
    }
    b->data[b->len++] = v;
}

/* The number after the prefix of a name like "n12" or "cluster_3". */
static double suffix_number(const char *name, size_t prefix) {
    return strlen(name) > prefix ? atof(name + prefix) : -1;
}

static void push_clusters(Buf *b, graph_t *g, double top) {
    for (int i = 1; i <= GD_n_cluster(g); i++) {
        graph_t *c = GD_clust(g)[i];
        boxf bb = GD_bb(c);
        push(b, suffix_number(agnameof(c), 8));
        push(b, bb.LL.x);
        push(b, top - bb.UR.y);
        push(b, bb.UR.x);
        push(b, top - bb.LL.y);
        push_clusters(b, c, top);
    }
}

static size_t count_clusters(graph_t *g) {
    size_t n = (size_t)GD_n_cluster(g);
    for (int i = 1; i <= GD_n_cluster(g); i++) {
        n += count_clusters(GD_clust(g)[i]);
    }
    return n;
}

static void push_label(Buf *b, textlabel_t *l, double top) {
    if (l && l->set) {
        push(b, 1);
        push(b, l->pos.x);
        push(b, top - l->pos.y);
    } else {
        push(b, 0);
        push(b, 0);
        push(b, 0);
    }
}

/* Lays out `dot` (a DOT graph) and returns the result as doubles in *out, y pointing
 * down, in points:
 *
 *   bb: x0 y0 x1 y1, node count, cluster count, edge count
 *   per node:    id x y width height          id is the number after "n" in its name
 *   per cluster: id x0 y0 x1 y1               id is the number after "cluster_"
 *   per edge:    id, 3 x (set x y) for the label, tail label and head label,
 *                number of curves, then per curve: number of points, x y pairs
 *                (cubic Bezier control points, 3k+1 of them); id is the edge's
 *                `comment` attribute
 *
 * Returns 0 on success; on failure the message is in *err. Free *out with umlaut_free. */
int umlaut_layout(const char *dot, double **out, size_t *len, const char **err) {
    error_text[0] = '\0';
    agseterrf(collect_error);
    *out = NULL;
    *len = 0;
    *err = error_text;

    GVC_t *gvc = gvContextPlugins(builtins, 0);
    graph_t *g = agmemread(dot);
    if (!g) {
        if (!error_text[0]) {
            snprintf(error_text, sizeof error_text, "could not parse the graph");
        }
        gvFreeContext(gvc);
        return 1;
    }
    if (gvLayout(gvc, g, "dot") != 0) {
        if (!error_text[0]) {
            snprintf(error_text, sizeof error_text, "dot could not lay out the graph");
        }
        agclose(g);
        gvFreeContext(gvc);
        return 1;
    }

    Buf b = {0};
    boxf bb = GD_bb(g);
    double top = bb.UR.y;
    size_t nodes = 0, edges = 0;
    for (node_t *n = agfstnode(g); n; n = agnxtnode(g, n)) {
        nodes++;
        for (edge_t *e = agfstout(g, n); e; e = agnxtout(g, e)) {
            edges++;
        }
    }
    push(&b, bb.LL.x);
    push(&b, 0);
    push(&b, bb.UR.x);
    push(&b, top - bb.LL.y);
    push(&b, (double)nodes);
    push(&b, (double)count_clusters(g));
    push(&b, (double)edges);

    for (node_t *n = agfstnode(g); n; n = agnxtnode(g, n)) {
        push(&b, suffix_number(agnameof(n), 1));
        push(&b, ND_coord(n).x);
        push(&b, top - ND_coord(n).y);
        push(&b, ND_width(n) * 72.0);
        push(&b, ND_height(n) * 72.0);
    }
    push_clusters(&b, g, top);
    for (node_t *n = agfstnode(g); n; n = agnxtnode(g, n)) {
        for (edge_t *e = agfstout(g, n); e; e = agnxtout(g, e)) {
            const char *id = agget(e, "comment");
            push(&b, id ? atof(id) : -1);
            push_label(&b, ED_label(e), top);
            push_label(&b, ED_tail_label(e), top);
            push_label(&b, ED_head_label(e), top);
            splines *spl = ED_spl(e);
            push(&b, spl ? (double)spl->size : 0);
            for (size_t i = 0; spl && i < spl->size; i++) {
                bezier *bz = &spl->list[i];
                push(&b, (double)bz->size);
                for (size_t k = 0; k < bz->size; k++) {
                    push(&b, bz->list[k].x);
                    push(&b, top - bz->list[k].y);
                }
            }
        }
    }

    gvFreeLayout(gvc, g);
    agclose(g);
    gvFreeContext(gvc);
    *out = b.data;
    *len = b.len;
    return 0;
}

void umlaut_free(double *p) {
    free(p);
}
