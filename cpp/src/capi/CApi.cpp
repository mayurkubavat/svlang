#include "svlang_lint/model/CApi.h"

#include <sstream>
#include <string>
#include <vector>

#include "svlang_lint/model/Session.h"

// The C enums mirror the C++ ones by value.
static_assert(static_cast<int>(svlang_lint::model::Direction::Ref) == SVLANG_LINT_DIR_REF);
static_assert(static_cast<int>(svlang_lint::model::TypeShape::UnpackedStruct) == SVLANG_LINT_SHAPE_UNPACKED_STRUCT);
static_assert(static_cast<int>(svlang_lint::model::UvmKind::Subscriber) == SVLANG_LINT_UVM_SUBSCRIBER);
static_assert(static_cast<int>(svlang_lint::model::RootKind::Property) == SVLANG_LINT_ROOT_PROPERTY);

using namespace svlang_lint::model;

struct svlang_lint_design {
    std::unique_ptr<Session> session;
    std::string error;
    std::string json;
};

namespace {

svlang_lint_loc toC(const Loc& l) { return {l.file, l.line, l.column, l.expFile, l.expLine}; }

const Model* modelOf(const svlang_lint_design* d) {
    return d && d->session && d->session->ok() ? &d->session->model() : nullptr;
}

} // namespace

extern "C" {

uint32_t svlang_lint_model_abi_version(void) { return SVLANG_LINT_MODEL_ABI_VERSION; }

svlang_lint_design* svlang_lint_design_load(int argc, const char* const* argv) {
    auto* d = new svlang_lint_design();
    try {
        std::vector<std::string> args;
        for (int i = 0; i < argc; ++i) args.emplace_back(argv[i] ? argv[i] : "");
        d->session = Session::load(args);
        if (!d->session->ok()) d->error = d->session->error();
    } catch (const std::exception& e) {
        d->error = e.what();
    } catch (...) {
        d->error = "unknown error";
    }
    return d;
}

void svlang_lint_design_free(svlang_lint_design* d) { delete d; }

const char* svlang_lint_design_error(const svlang_lint_design* d) {
    if (!d) return "null design";
    return d->error.empty() ? nullptr : d->error.c_str();
}

uint32_t svlang_lint_design_compile_errors(const svlang_lint_design* d) {
    return modelOf(d) ? static_cast<uint32_t>(d->session->errorCount()) : 0;
}

double svlang_lint_design_time_ms(const svlang_lint_design* d, int stage) {
    if (!modelOf(d)) return 0;
    const auto& t = d->session->timings();
    switch (stage) {
        case 0: return t.parseMs;
        case 1: return t.elaborateMs;
        case 2: return t.modelMs;
        default: return 0;
    }
}

uint32_t svlang_lint_file_count(const svlang_lint_design* d) {
    const Model* m = modelOf(d);
    return m ? static_cast<uint32_t>(m->files().size()) : 0;
}

const char* svlang_lint_file_name(const svlang_lint_design* d, uint32_t file) {
    const Model* m = modelOf(d);
    if (!m || file >= m->files().size()) return nullptr;
    return m->files()[file].c_str();
}

uint32_t svlang_lint_class_count(const svlang_lint_design* d) {
    const Model* m = modelOf(d);
    return m ? static_cast<uint32_t>(m->classes().size()) : 0;
}

int svlang_lint_get_class(const svlang_lint_design* d, uint32_t cls, svlang_lint_class* out) {
    const Model* m = modelOf(d);
    if (!m || !out || cls >= m->classes().size()) return 0;
    const Class& c = m->classes()[cls];
    *out = {c.name.c_str(), c.scope.c_str(), c.baseName.c_str(), c.base, static_cast<uint8_t>(c.uvm),
            c.isVirtual, c.isParameterized, c.isLibrary, toC(c.loc),
            static_cast<uint32_t>(c.properties.size()), static_cast<uint32_t>(c.methods.size()),
            static_cast<uint32_t>(c.constraints.size()), static_cast<uint32_t>(c.macros.size()),
            static_cast<uint32_t>(c.typedefs.size()), c.isInterface};
    return 1;
}

int svlang_lint_get_property(const svlang_lint_design* d, uint32_t cls, uint32_t index, svlang_lint_property* out) {
    const Model* m = modelOf(d);
    if (!m || !out || cls >= m->classes().size()) return 0;
    const auto& props = m->classes()[cls].properties;
    if (index >= props.size()) return 0;
    const Property& p = props[index];
    *out = {p.name.c_str(), p.type.c_str(), p.typeClass, static_cast<uint8_t>(p.visibility),
            p.isStatic, p.isConst, p.isRand, p.isVirtualInterface, toC(p.loc)};
    return 1;
}

int svlang_lint_get_method(const svlang_lint_design* d, uint32_t cls, uint32_t index, svlang_lint_method* out) {
    const Model* m = modelOf(d);
    if (!m || !out || cls >= m->classes().size()) return 0;
    const auto& methods = m->classes()[cls].methods;
    if (index >= methods.size()) return 0;
    const Method& x = methods[index];
    *out = {x.name.c_str(), static_cast<uint8_t>(x.visibility), x.isTask, x.isVirtual, x.isPure,
            x.isStatic, static_cast<uint8_t>(x.phase), x.declaredVirtual, x.isBuiltIn, x.isExtern,
            x.returnsVoid, x.returnType.c_str(), toC(x.loc),
            static_cast<uint32_t>(x.arguments.size()), static_cast<uint32_t>(x.locals.size()),
            static_cast<uint32_t>(x.calls.size()), static_cast<uint32_t>(x.assignments.size()),
            static_cast<uint32_t>(x.timingControls.size()), x.superNewIndex, toC(x.superNewLoc),
            x.callsSuperNew};
    return 1;
}

static const Method* methodOf(const svlang_lint_design* d, uint32_t cls, uint32_t method) {
    const Model* m = modelOf(d);
    if (!m || cls >= m->classes().size()) return nullptr;
    const auto& methods = m->classes()[cls].methods;
    return method < methods.size() ? &methods[method] : nullptr;
}

int svlang_lint_get_argument(const svlang_lint_design* d, uint32_t cls, uint32_t method, uint32_t index,
                          svlang_lint_argument* out) {
    const Method* x = methodOf(d, cls, method);
    if (!x || !out || index >= x->arguments.size()) return 0;
    const Argument& a = x->arguments[index];
    *out = {a.name.c_str(), a.type.c_str(), a.canonicalType.c_str(), a.typeClass,
            static_cast<uint8_t>(a.direction), static_cast<uint8_t>(a.shape), toC(a.loc)};
    return 1;
}

int svlang_lint_get_local(const svlang_lint_design* d, uint32_t cls, uint32_t method, uint32_t index,
                       svlang_lint_local* out) {
    const Method* x = methodOf(d, cls, method);
    if (!x || !out || index >= x->locals.size()) return 0;
    const Local& l = x->locals[index];
    *out = {l.name.c_str(), l.type.c_str(), l.isVirtualInterface, toC(l.loc)};
    return 1;
}

int svlang_lint_get_call(const svlang_lint_design* d, uint32_t cls, uint32_t method, uint32_t index,
                      svlang_lint_call* out) {
    const Method* x = methodOf(d, cls, method);
    if (!x || !out || index >= x->calls.size()) return 0;
    const Call& c = x->calls[index];
    *out = {c.name.c_str(), c.calleeScope.c_str(), c.receiver.c_str(), toC(c.loc), toC(c.firstArgLoc),
            c.isSystem, c.calleeDeclaredVirtual, c.calleeBuiltIn, c.firstArgIsNull, c.viaSuper};
    return 1;
}

int svlang_lint_get_assignment(const svlang_lint_design* d, uint32_t cls, uint32_t method, uint32_t index,
                            svlang_lint_assignment* out) {
    const Method* x = methodOf(d, cls, method);
    if (!x || !out || index >= x->assignments.size()) return 0;
    const Assignment& a = x->assignments[index];
    *out = {a.lhs.c_str(), a.rhs.c_str(), a.rootName.c_str(), toC(a.loc), a.isNonBlocking,
            static_cast<uint8_t>(a.rootKind)};
    return 1;
}

int svlang_lint_get_timing(const svlang_lint_design* d, uint32_t cls, uint32_t method, uint32_t index,
                        svlang_lint_loc* out) {
    const Method* x = methodOf(d, cls, method);
    if (!x || !out || index >= x->timingControls.size()) return 0;
    *out = toC(x->timingControls[index]);
    return 1;
}

int svlang_lint_get_constraint(const svlang_lint_design* d, uint32_t cls, uint32_t index, svlang_lint_constraint* out) {
    const Model* m = modelOf(d);
    if (!m || !out || cls >= m->classes().size()) return 0;
    const auto& cs = m->classes()[cls].constraints;
    if (index >= cs.size()) return 0;
    *out = {cs[index].name.c_str(), toC(cs[index].loc)};
    return 1;
}

int svlang_lint_get_macro(const svlang_lint_design* d, uint32_t cls, uint32_t index, svlang_lint_macro* out) {
    const Model* m = modelOf(d);
    if (!m || !out || cls >= m->classes().size()) return 0;
    const auto& xs = m->classes()[cls].macros;
    if (index >= xs.size()) return 0;
    *out = {xs[index].name.c_str(), toC(xs[index].loc)};
    return 1;
}

int svlang_lint_get_typedef(const svlang_lint_design* d, uint32_t cls, uint32_t index, svlang_lint_typedef* out) {
    const Model* m = modelOf(d);
    if (!m || !out || cls >= m->classes().size()) return 0;
    const auto& xs = m->classes()[cls].typedefs;
    if (index >= xs.size()) return 0;
    *out = {xs[index].name.c_str(), toC(xs[index].loc)};
    return 1;
}

const char* svlang_lint_design_json(svlang_lint_design* d) {
    const Model* m = modelOf(d);
    if (!m) return nullptr;
    if (d->json.empty()) {
        std::ostringstream out;
        m->writeJson(out);
        d->json = out.str();
    }
    return d->json.c_str();
}

} // extern "C"
