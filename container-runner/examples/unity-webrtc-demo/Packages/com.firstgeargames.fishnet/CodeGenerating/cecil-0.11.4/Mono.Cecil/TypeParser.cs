//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Cecil.Metadata;
using System;
using System.Text;
using MonoFN.Collections.Generic;

namespace MonoFN.Cecil
{
    internal class TypeParser
    {
        private class Type
        {
            public const int Ptr = -1;
            public const int ByRef = -2;
            public const int SzArray = -3;
            public string type_fullname;
            public string[] nested_names;
            public int arity;
            public int[] specs;
            public Type[] generic_arguments;
            public string assembly;
        }

        private readonly string fullname;
        private readonly int length;
        private int position;

        private TypeParser(string fullname)
        {
            this.fullname = fullname;
            length = fullname.Length;
        }

        private Type ParseType(bool fq_name)
        {
            Type type = new();
            type.type_fullname = ParsePart();

            type.nested_names = ParseNestedNames();

            if (TryGetArity(type))
                type.generic_arguments = ParseGenericArguments(type.arity);

            type.specs = ParseSpecs();

            if (fq_name)
                type.assembly = ParseAssemblyName();

            return type;
        }

        private static bool TryGetArity(Type type)
        {
            int arity = 0;

            TryAddArity(type.type_fullname, ref arity);

            string[] nested_names = type.nested_names;
            if (!nested_names.IsNullOrEmpty())
            {
                for (int i = 0; i < nested_names.Length; i++)
                    TryAddArity(nested_names[i], ref arity);
            }

            type.arity = arity;
            return arity > 0;
        }

        private static bool TryGetArity(string name, out int arity)
        {
            arity = 0;
            int index = name.LastIndexOf('`');
            if (index == -1)
                return false;

            return ParseInt32(name.Substring(index + 1), out arity);
        }

        private static bool ParseInt32(string value, out int result)
        {
            return int.TryParse(value, out result);
        }

        private static void TryAddArity(string name, ref int arity)
        {
            int type_arity;
            if (!TryGetArity(name, out type_arity))
                return;

            arity += type_arity;
        }

        private string ParsePart()
        {
            StringBuilder part = new();
            while (position < length && !IsDelimiter(fullname[position]))
            {
                if (fullname[position] == '\\')
                    position++;

                part.Append(fullname[position++]);
            }

            return part.ToString();
        }

        private static bool IsDelimiter(char chr)
        {
            return "+,[]*&".IndexOf(chr) != -1;
        }

        private void TryParseWhiteSpace()
        {
            while (position < length && Char.IsWhiteSpace(fullname[position]))
                position++;
        }

        private string[] ParseNestedNames()
        {
            string[] nested_names = null;
            while (TryParse('+'))
                Add(ref nested_names, ParsePart());

            return nested_names;
        }

        private bool TryParse(char chr)
        {
            if (position < length && fullname[position] == chr)
            {
                position++;
                return true;
            }

            return false;
        }

        private static void Add<T>(ref T[] array, T item)
        {
            array = array.Add(item);
        }

        private int[] ParseSpecs()
        {
            int[] specs = null;

            while (position < length)
            {
                switch (fullname[position])
                {
                    case '*':
                        position++;
                        Add(ref specs, Type.Ptr);
                        break;
                    case '&':
                        position++;
                        Add(ref specs, Type.ByRef);
                        break;
                    case '[':
                        position++;
                        switch (fullname[position])
                        {
                            case ']':
                                position++;
                                Add(ref specs, Type.SzArray);
                                break;
                            case '*':
                                position++;
                                Add(ref specs, 1);
                                break;
                            default:
                                int rank = 1;
                                while (TryParse(','))
                                    rank++;

                                Add(ref specs, rank);

                                TryParse(']');
                                break;
                        }
                        break;
                    default:
                        return specs;
                }
            }

            return specs;
        }

        private Type[] ParseGenericArguments(int arity)
        {
            Type[] generic_arguments = null;

            if (position == length || fullname[position] != '[')
                return generic_arguments;

            TryParse('[');

            for (int i = 0; i < arity; i++)
            {
                bool fq_argument = TryParse('[');
                Add(ref generic_arguments, ParseType(fq_argument));
                if (fq_argument)
                    TryParse(']');

                TryParse(',');
                TryParseWhiteSpace();
            }

            TryParse(']');

            return generic_arguments;
        }

        private string ParseAssemblyName()
        {
            if (!TryParse(','))
                return string.Empty;

            TryParseWhiteSpace();

            int start = position;
            while (position < length)
            {
                char chr = fullname[position];
                if (chr == '[' || chr == ']')
                    break;

                position++;
            }

            return fullname.Substring(start, position - start);
        }

        public static TypeReference ParseType(ModuleDefinition module, string fullname, bool typeDefinitionOnly = false)
        {
            if (string.IsNullOrEmpty(fullname))
                return null;

            TypeParser parser = new(fullname);
            return GetTypeReference(module, parser.ParseType(true), typeDefinitionOnly);
        }

        private static TypeReference GetTypeReference(ModuleDefinition module, Type type_info, bool type_def_only)
        {
            TypeReference type;
            if (!TryGetDefinition(module, type_info, out type))
            {
                if (type_def_only)
                    return null;

                type = CreateReference(type_info, module, GetMetadataScope(module, type_info));
            }

            return CreateSpecs(type, type_info);
        }

        private static TypeReference CreateSpecs(TypeReference type, Type type_info)
        {
            type = TryCreateGenericInstanceType(type, type_info);

            int[] specs = type_info.specs;
            if (specs.IsNullOrEmpty())
                return type;

            for (int i = 0; i < specs.Length; i++)
            {
                switch (specs[i])
                {
                    case Type.Ptr:
                        type = new PointerType(type);
                        break;
                    case Type.ByRef:
                        type = new ByReferenceType(type);
                        break;
                    case Type.SzArray:
                        type = new ArrayType(type);
                        break;
                    default:
                        ArrayType array = new(type);
                        array.Dimensions.Clear();

                        for (int j = 0; j < specs[i]; j++)
                            array.Dimensions.Add(new());

                        type = array;
                        break;
                }
            }

            return type;
        }

        private static TypeReference TryCreateGenericInstanceType(TypeReference type, Type type_info)
        {
            Type[] generic_arguments = type_info.generic_arguments;
            if (generic_arguments.IsNullOrEmpty())
                return type;

            GenericInstanceType instance = new(type, generic_arguments.Length);
            Collection<TypeReference> instance_arguments = instance.GenericArguments;

            for (int i = 0; i < generic_arguments.Length; i++)
                instance_arguments.Add(GetTypeReference(type.Module, generic_arguments[i], false));

            return instance;
        }

        public static void SplitFullName(string fullname, out string @namespace, out string name)
        {
            int last_dot = fullname.LastIndexOf('.');

            if (last_dot == -1)
            {
                @namespace = string.Empty;
                name = fullname;
            }
            else
            {
                @namespace = fullname.Substring(0, last_dot);
                name = fullname.Substring(last_dot + 1);
            }
        }

        private static TypeReference CreateReference(Type type_info, ModuleDefinition module, IMetadataScope scope)
        {
            string @namespace, name;
            SplitFullName(type_info.type_fullname, out @namespace, out name);

            TypeReference type = new(@namespace, name, module, scope);
            MetadataSystem.TryProcessPrimitiveTypeReference(type);

            AdjustGenericParameters(type);

            string[] nested_names = type_info.nested_names;
            if (nested_names.IsNullOrEmpty())
                return type;

            for (int i = 0; i < nested_names.Length; i++)
            {
                type = new(string.Empty, nested_names[i], module, null)
                {
                    DeclaringType = type
                };

                AdjustGenericParameters(type);
            }

            return type;
        }

        private static void AdjustGenericParameters(TypeReference type)
        {
            int arity;
            if (!TryGetArity(type.Name, out arity))
                return;

            for (int i = 0; i < arity; i++)
                type.GenericParameters.Add(new(type));
        }

        private static IMetadataScope GetMetadataScope(ModuleDefinition module, Type type_info)
        {
            if (string.IsNullOrEmpty(type_info.assembly))
                return module.TypeSystem.CoreLibrary;

            AssemblyNameReference match;
            AssemblyNameReference reference = AssemblyNameReference.Parse(type_info.assembly);

            return module.TryGetAssemblyNameReference(reference, out match) ? match : reference;
        }

        private static bool TryGetDefinition(ModuleDefinition module, Type type_info, out TypeReference type)
        {
            type = null;
            if (!TryCurrentModule(module, type_info))
                return false;

            TypeDefinition typedef = module.GetType(type_info.type_fullname);
            if (typedef == null)
                return false;

            string[] nested_names = type_info.nested_names;
            if (!nested_names.IsNullOrEmpty())
            {
                for (int i = 0; i < nested_names.Length; i++)
                {
                    TypeDefinition nested_type = typedef.GetNestedType(nested_names[i]);
                    if (nested_type == null)
                        return false;

                    typedef = nested_type;
                }
            }

            type = typedef;
            return true;
        }

        private static bool TryCurrentModule(ModuleDefinition module, Type type_info)
        {
            if (string.IsNullOrEmpty(type_info.assembly))
                return true;

            if (module.assembly != null && module.assembly.Name.FullName == type_info.assembly)
                return true;

            return false;
        }

        public static string ToParseable(TypeReference type, bool top_level = true)
        {
            if (type == null)
                return null;

            StringBuilder name = new();
            AppendType(type, name, true, top_level);
            return name.ToString();
        }

        private static void AppendNamePart(string part, StringBuilder name)
        {
            foreach (char c in part)
            {
                if (IsDelimiter(c))
                    name.Append('\\');

                name.Append(c);
            }
        }

        private static void AppendType(TypeReference type, StringBuilder name, bool fq_name, bool top_level)
        {
            TypeReference element_type = type.GetElementType();

            TypeReference declaring_type = element_type.DeclaringType;
            if (declaring_type != null)
            {
                AppendType(declaring_type, name, false, top_level);
                name.Append('+');
            }

            string @namespace = type.Namespace;
            if (!string.IsNullOrEmpty(@namespace))
            {
                AppendNamePart(@namespace, name);
                name.Append('.');
            }

            AppendNamePart(element_type.Name, name);

            if (!fq_name)
                return;

            if (type.IsTypeSpecification())
                AppendTypeSpecification((TypeSpecification)type, name);

            if (RequiresFullyQualifiedName(type, top_level))
            {
                name.Append(", ");
                name.Append(GetScopeFullName(type));
            }
        }

        private static string GetScopeFullName(TypeReference type)
        {
            IMetadataScope scope = type.Scope;
            switch (scope.MetadataScopeType)
            {
                case MetadataScopeType.AssemblyNameReference:
                    return ((AssemblyNameReference)scope).FullName;
                case MetadataScopeType.ModuleDefinition:
                    return ((ModuleDefinition)scope).Assembly.Name.FullName;
            }

            throw new ArgumentException();
        }

        private static void AppendTypeSpecification(TypeSpecification type, StringBuilder name)
        {
            if (type.ElementType.IsTypeSpecification())
                AppendTypeSpecification((TypeSpecification)type.ElementType, name);

            switch (type.etype)
            {
                case ElementType.Ptr:
                    name.Append('*');
                    break;
                case ElementType.ByRef:
                    name.Append('&');
                    break;
                case ElementType.SzArray:
                case ElementType.Array:
                    ArrayType array = (ArrayType)type;
                    if (array.IsVector)
                    {
                        name.Append("[]");
                    }
                    else
                    {
                        name.Append('[');
                        for (int i = 1; i < array.Rank; i++)
                            name.Append(',');
                        name.Append(']');
                    }
                    break;
                case ElementType.GenericInst:
                    GenericInstanceType instance = (GenericInstanceType)type;
                    Collection<TypeReference> arguments = instance.GenericArguments;

                    name.Append('[');

                    for (int i = 0; i < arguments.Count; i++)
                    {
                        if (i > 0)
                            name.Append(',');

                        TypeReference argument = arguments[i];
                        bool requires_fqname = argument.Scope != argument.Module;

                        if (requires_fqname)
                            name.Append('[');

                        AppendType(argument, name, true, false);

                        if (requires_fqname)
                            name.Append(']');
                    }

                    name.Append(']');
                    break;
                default:
                    return;
            }
        }

        private static bool RequiresFullyQualifiedName(TypeReference type, bool top_level)
        {
            if (type.Scope == type.Module)
                return false;

            if (type.Scope.Name == "mscorlib" && top_level)
                return false;

            return true;
        }
    }
}