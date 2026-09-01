using MonoFN.Cecil;
using SR = System.Reflection;

namespace FishNet.CodeGenerating
{
    internal abstract class CodegenBase
    {
        // Lazy debug checks.
        public bool IsIsolatedAsm => Module.Name.Contains("IsolatedAsm");
        public bool IsRuntimeAsm => Module.Name.Contains("FishNet.Runtime");
        public CodegenSession Session { get; private set; }
        public ModuleDefinition Module { get; private set; }

        public virtual bool ImportReferences()
        {
            return true;
        }

        public void Initialize(CodegenSession session)
        {
            Session = session;
            Module = session.Module;
        }

        /// <summary>
        /// Returns class of type if found within Session.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        internal T GetClass<T>() where T : CodegenBase => Session.GetClass<T>();

        public string MethodDefinitionTraceText(MethodDefinition methodDef) => Session.MethodDefinitionTraceText(methodDef);
        public string TypeDefinitionTraceText(TypeDefinition typeDef) => Session.TypeDefinitionTraceText(typeDef);
        public string TypeReferenceTraceText(TypeReference typeRef) => Session.TypeReferenceTraceText(typeRef);
        public string FieldDefinitionTraceText(FieldDefinition fieldDef) => Session.FieldDefinitionTraceText(fieldDef);
        public string PropertyDefinitionTraceText(PropertyDefinition propertyDef) => Session.PropertyDefinitionTraceText(propertyDef);

        #region Logging.
        /// <summary>
        /// Logs a warning.
        /// </summary>
        /// <param name = "msg"></param>
        internal void LogWarning(string msg) => Session.LogWarning(msg);

        /// <summary>
        /// Logs an error.
        /// </summary>
        /// <param name = "msg"></param>
        internal void LogError(string msg) => Session.LogError(msg);
        #endregion

        #region ImportReference.
        public MethodReference ImportReference(SR.MethodBase method) => Session.ImportReference(method);
        public MethodReference ImportReference(SR.MethodBase method, IGenericParameterProvider context) => Session.ImportReference(method, context);
        public TypeReference ImportReference(TypeReference type) => Session.ImportReference(type);
        public TypeReference ImportReference(TypeReference type, IGenericParameterProvider context) => Session.ImportReference(type, context);
        public FieldReference ImportReference(FieldReference field) => Session.ImportReference(field);
        public FieldReference ImportReference(FieldReference field, IGenericParameterProvider context) => Session.ImportReference(field, context);
        public FieldReference ImportReference(SR.FieldInfo field) => Session.ImportReference(field);
        public FieldReference ImportReference(SR.FieldInfo field, IGenericParameterProvider context) => Session.ImportReference(field, context);
        public MethodReference ImportReference(MethodReference method) => Session.ImportReference(method);
        public MethodReference ImportReference(MethodReference method, IGenericParameterProvider context) => Session.ImportReference(method, context);
        public TypeReference ImportReference(System.Type type) => Session.ImportReference(type, null);
        public TypeReference ImportReference(System.Type type, IGenericParameterProvider context) => Session.ImportReference(type, context);
        #endregion
    }
}