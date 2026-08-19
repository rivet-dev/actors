using MonoFN.Cecil;
using System.Linq;

namespace FishNet.CodeGenerating.Helping.Extension
{
   internal static class CustomAttributeExtensions
    {
        private static IGenericParameterProvider GetSafeContext(CodegenSession session, IGenericParameterProvider context)
        {
            return (context != null && context.Module == session.Module) ? context : null;
        }

        /// <summary>
        /// Clones an attribute into the current session module.
        /// </summary>
        internal static CustomAttribute CloneImported(this CustomAttribute customAttr, CodegenSession session, IGenericParameterProvider context = null)
        {
            IGenericParameterProvider safeContext = GetSafeContext(session, context);
            MethodReference ctor = (safeContext == null) ? session.ImportReference(customAttr.Constructor) : session.ImportReference(customAttr.Constructor, safeContext);
            CustomAttribute result = new(ctor);

            foreach (CustomAttributeArgument item in customAttr.ConstructorArguments)
                result.ConstructorArguments.Add(item.CloneImported(session, safeContext));

            foreach (CustomAttributeNamedArgument item in customAttr.Fields)
                result.Fields.Add(item.CloneImported(session, safeContext));

            foreach (CustomAttributeNamedArgument item in customAttr.Properties)
                result.Properties.Add(item.CloneImported(session, safeContext));

            return result;
        }

        /// <summary>
        /// Clones an attribute argument into the current session module.
        /// </summary>
        internal static CustomAttributeArgument CloneImported(this CustomAttributeArgument customAttrArg, CodegenSession session, IGenericParameterProvider context = null)
        {
            IGenericParameterProvider safeContext = GetSafeContext(session, context);
            TypeReference typeRef = safeContext == null ? session.ImportReference(customAttrArg.Type) : session.ImportReference(customAttrArg.Type, safeContext);
            object value = customAttrArg.Value;

            if (value is TypeReference tr)
            {
                value = (safeContext == null) ? session.ImportReference(tr) : session.ImportReference(tr, safeContext);
            }
            else if (value is CustomAttributeArgument[] arguments)
            {
                CustomAttributeArgument[] clonedArguments = new CustomAttributeArgument[arguments.Length];
                for (int i = 0; i < arguments.Length; i++)
                    clonedArguments[i] = arguments[i].CloneImported(session, safeContext);

                value = clonedArguments;
            }

            return new(typeRef, value);
        }

        /// <summary>
        /// Clones a named attribute argument into the current session module.
        /// </summary>
        internal static CustomAttributeNamedArgument CloneImported(this CustomAttributeNamedArgument customAttrNamedArg, CodegenSession session, IGenericParameterProvider context = null)
        {
            return new(customAttrNamedArg.Name, customAttrNamedArg.Argument.CloneImported(session, context));
        }
        
        /// <summary>
        /// Finds a field within an attribute.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <param name = "customAttr"></param>
        /// <param name = "field"></param>
        /// <param name = "defaultValue"></param>
        /// <returns></returns>
        internal static T GetField<T>(this CustomAttribute customAttr, string field, T defaultValue)
        {
            foreach (CustomAttributeNamedArgument customField in customAttr.Fields)
            {
                if (customField.Name == field)
                {
                    return (T)customField.Argument.Value;
                }
            }

            return defaultValue;
        }

        /// <summary>
        /// Returns if any of the attributes match IAtrribute.
        /// </summary>
        /// <typeparam name = "TAttribute"></typeparam>
        /// <param name = "attributeProvider"></param>
        /// <returns></returns>
        internal static bool HasCustomAttribute<TAttribute>(this ICustomAttributeProvider attributeProvider)
        {
            return attributeProvider.CustomAttributes.Any(attr => attr.AttributeType.Is<TAttribute>());
        }

        /// <summary>
        /// Returns if ca is of type target.
        /// </summary>
        /// <param name = "ca"></param>
        /// <param name = "targetFullName"></param>
        /// <returns></returns>
        internal static bool Is(this CustomAttribute ca, string targetFullName)
        {
            return ca.AttributeType.FullName == targetFullName;
        }
    }
}