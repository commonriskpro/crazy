use super::AclSymbol;

pub(super) const ACL_SYMBOLS: &[AclSymbol] = &[
    AclSymbol {
        label: "change",
        detail: "ACL header",
        documentation: "Starts an ACL ChangeSet document.",
        insert_text: "change ${1:name}",
    },
    AclSymbol {
        label: "author",
        detail: "ACL metadata",
        documentation: "Declares the human or agent author of the ChangeSet.",
        insert_text: "author ${1:name}",
    },
    AclSymbol {
        label: "description",
        detail: "ACL metadata",
        documentation: "Describes the purpose of the ChangeSet.",
        insert_text: "description ${1:text}",
    },
    AclSymbol {
        label: "base",
        detail: "ACL snapshot guard",
        documentation: "Declares the expected base snapshot id.",
        insert_text: "base ${1:0}",
    },
    AclSymbol {
        label: "op create_function",
        detail: "Create a function node",
        documentation: "Creates a semantic function node. Requires id; return/body are optional but useful for executable code.",
        insert_text: "op create_function id=fn.${1:name} return=${2:Int} body=${3:add(20, 22)}",
    },
    AclSymbol {
        label: "op create_test",
        detail: "Create a test node",
        documentation: "Creates a semantic test node that `ail test` can discover and run.",
        insert_text: "op create_test id=test.${1:name} body=${2:eq(add(20, 22), 42)}",
    },
    AclSymbol {
        label: "op create_capability",
        detail: "Create a capability node",
        documentation: "Declares an external capability such as log.write.",
        insert_text: "op create_capability id=${1:log.write}",
    },
    AclSymbol {
        label: "op grant",
        detail: "Grant a capability requirement",
        documentation: "Adds a capability requirement to a target node.",
        insert_text: "op grant target=${1:fn.main} capability=${2:log.write}",
    },
    AclSymbol {
        label: "op set_body",
        detail: "Set function body",
        documentation: "Updates the body expression for an existing graph node.",
        insert_text: "op set_body target=${1:fn.main} body=${2:add(20, 22)}",
    },
    AclSymbol {
        label: "op add_param",
        detail: "Add function parameter",
        documentation: "Adds a typed parameter to a function node.",
        insert_text: "op add_param target=${1:fn.main} name=${2:x} type=${3:Int}",
    },
    AclSymbol {
        label: "end",
        detail: "ACL terminator",
        documentation: "Ends an ACL ChangeSet document.",
        insert_text: "end",
    },
];
