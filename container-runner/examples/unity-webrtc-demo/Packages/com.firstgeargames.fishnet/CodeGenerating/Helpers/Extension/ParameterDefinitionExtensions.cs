using MonoFN.Cecil;
using System;

namespace FishNet.CodeGenerating.Helping.Extension
{
    internal static class ParameterDefinitionExtensions
    {
        private static IGenericParameterProvider GetSafeContext(CodegenSession session, IGenericParameterProvider context)
        {
            return context != null && context.Module == session.Module ? context : null;
        }

        /// <summary>
        /// Returns if parameterDef is Type.
        /// </summary>
        /// <param name = "parameterDef"></param>
        /// <param name = "type"></param>
        /// <returns></returns>
        public static bool Is(this ParameterDefinition parameterDef, Type type)
        {
            return parameterDef.ParameterType.FullName == type.FullName;
        }

        /// <summary>
        /// Clones a parameter into the current session module.
        /// </summary>
        public static ParameterDefinition CloneImported(this ParameterDefinition parameterDef, CodegenSession session, IGenericParameterProvider context = null, string nameOverride = null)
        {
            IGenericParameterProvider safeContext = GetSafeContext(session, context);
            TypeReference parameterTypeRef = safeContext == null ? session.ImportReference(parameterDef.ParameterType) : session.ImportReference(parameterDef.ParameterType, safeContext);
            ParameterDefinition result = new(nameOverride ?? parameterDef.Name, parameterDef.Attributes, parameterTypeRef)
            {
                Constant = parameterDef.Constant,
                IsReturnValue = parameterDef.IsReturnValue,
                IsOut = parameterDef.IsOut,
                IsIn = parameterDef.IsIn,
                IsLcid = parameterDef.IsLcid,
                IsOptional = parameterDef.IsOptional,
                HasConstant = parameterDef.HasConstant,
                HasDefault = parameterDef.HasDefault,
                HasFieldMarshal = parameterDef.HasFieldMarshal
            };

            if (parameterDef.HasMarshalInfo)
                result.MarshalInfo = parameterDef.MarshalInfo;

            foreach (CustomAttribute item in parameterDef.CustomAttributes)
                result.CustomAttributes.Add(item.CloneImported(session, safeContext));

            return result;
        }
    }
}