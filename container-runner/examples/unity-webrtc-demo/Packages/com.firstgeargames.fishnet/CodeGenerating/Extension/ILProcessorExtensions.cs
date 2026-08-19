using FishNet.CodeGenerating.Helping.Extension;
using MonoFN.Cecil;
using MonoFN.Cecil.Cil;

namespace FishNet.CodeGenerating.Extension
{
    internal static class ILProcessorExtensions
    {
        /// <summary>
        /// Creates a variable type within the body and returns it's VariableDef.
        /// </summary>
        internal static VariableDefinition CreateVariable(this ILProcessor processor, CodegenSession session, System.Type variableType)
        {
            return processor.Body.Method.CreateVariable(session, variableType);
        }

        /// <summary>
        /// Creates a variable type within the body and returns it's VariableDef.
        /// </summary>
        internal static VariableDefinition CreateVariable(this ILProcessor processor, CodegenSession session, TypeReference variableTr)
        {
            return processor.Body.Method.CreateVariable(variableTr);
        }
    }
}