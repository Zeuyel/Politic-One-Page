import os
import time
import json
from typing import List, Optional
from scraper import fetch_all_errors, set_runtime_options, get_runtime_options
from data_manager import load_data, save_data, update_question_status, ErrorData, Question, DATA_FILE

def display_question_summary(question: Question):
    """Displays a summary of a question."""
    status_icon = "✅" if question.user_status == "mastered" else "🔄" if question.user_status == "reviewing" else "🆕"
    print(f"  ID: {question.id} | {status_icon} {question.user_status.capitalize()} | {question.origin_name} - {question.sub_name}")
    print(f"    Content: {question.content[:70]}...")

def display_question_details(question: Question):
    """Displays full details of a question."""
    print(f"\n--- Question Details (ID: {question.id}) ---")
    print(f"Source: {question.origin_name} - {question.sub_name}")
    print(f"Status: {question.user_status.capitalize()} | Last Reviewed: {question.last_reviewed or 'Never'}")
    print("\nContent:")
    print(question.content)
    print("\nOptions:")
    for opt in question.options:
        print(f"  {opt.label}. {opt.content}")
    
    input("\nPress Enter to reveal answer and analysis...")
    
    print("\n--- Answer & Analysis ---")
    print(f"Correct Answer(s): {', '.join(question.answer)}")
    print("\nAnalysis:")
    print(question.analysis)
    if question.comments:
        print("\nComments:")
        for comment in question.comments:
            print(f"  - {comment}")
    else:
        print("\nNo comments available.")

def sync_data():
    """Triggers data synchronization and saves to file."""
    print("Starting data synchronization...")
    try:
        # fetch_all_errors already handles printing progress
        new_data_dict = fetch_all_errors()
        # 同步数量汇总
        sim_n = len(new_data_dict.get('simulation', []))
        real_n = len(new_data_dict.get('real', []))
        fam_n = len(new_data_dict.get('famous', []))
        print(f"Summary -> simulation: {sim_n}, real: {real_n}, famous: {fam_n}")
        
        # Convert dict to ErrorData object for saving
        # The data_manager.load_data already handles parsing, but for new_data_dict
        # we need to create the object from it manually if we want to use save_data(ErrorData)
        
        # Simpler: just save the dict directly like scraper.py does in main block
        os.makedirs(os.path.dirname(DATA_FILE), exist_ok=True)
        with open(DATA_FILE, 'w', encoding='utf-8') as f:
            json.dump(new_data_dict, f, ensure_ascii=False, indent=2)
            
        print("Data synchronization complete. Data saved.")
        if (sim_n + real_n + fam_n) == 0:
            print("Warning: No questions fetched. Check TOKEN validity or network, or set INCLUDE_COMMENTS=0 to speed up.")
        return True
    except Exception as e:
        print(f"Error during synchronization: {e}")
        return False

def settings_menu():
    """交互式设置抓取参数（评论、批大小、TOKEN）。"""
    while True:
        opts = get_runtime_options()
        print("\n--- Settings ---")
        print(f"1. INCLUDE_COMMENTS: {'ON' if opts['include_comments'] else 'OFF'}")
        print(f"2. BATCH_SIZE: {opts['batch_size']}")
        print(f"3. TOKEN: {'SET' if opts['token_set'] else 'NOT SET'}")
        print("4. 返回主菜单")
        choice = input("选择要修改的项: ").strip()

        if choice == '1':
            val = input("是否抓取评论? (y/n): ").strip().lower()
            include = 1 if val in ('y', 'yes', '1') else 0
            set_runtime_options(include_comments=include)
            print("INCLUDE_COMMENTS 已更新。")
        elif choice == '2':
            val = input("请输入批大小（建议 50-80）: ").strip()
            try:
                bs = int(val)
                if bs <= 0:
                    raise ValueError
                set_runtime_options(batch_size=bs)
                print("BATCH_SIZE 已更新。")
            except Exception:
                print("无效的数字。")
        elif choice == '3':
            token = input("请输入 TOKEN（不会回显历史）: ").strip()
            if token:
                set_runtime_options(token=token)
                print("TOKEN 已更新。")
            else:
                print("未输入 TOKEN。")
        elif choice == '4':
            break
        else:
            print("无效选择。")

def list_questions(data: ErrorData):
    """Lists questions with optional filtering."""
    print("\n--- Listing Questions ---")
    all_questions: List[Question] = []
    # 运行时为每个题目标注来源类别，兼容旧数据
    for q in data.simulation:
        try:
            q.source = q.source or 'simulation'
        except Exception:
            pass
        all_questions.append(q)
    for q in data.real:
        try:
            q.source = q.source or 'real'
        except Exception:
            pass
        all_questions.append(q)
    for q in data.famous:
        try:
            q.source = q.source or 'famous'
        except Exception:
            pass
        all_questions.append(q)

    if not all_questions:
        print("No questions loaded. Try syncing data first.")
        return

    while True:
        print("\nFilter options:")
        print("  1. All questions")
        print("  2. By source (simulation, real, famous)")
        print("  3. By status (new, reviewing, mastered)")
        print("  4. Back to main menu")
        choice = input("Enter your choice: ").strip()

        filtered_questions = all_questions
        source_filter: Optional[str] = None
        status_filter: Optional[str] = None

        if choice == '2':
            source_filter = input("Enter source (simulation, real, famous): ").strip().lower()
            if source_filter not in ("simulation", "real", "famous"):
                print("Invalid source. Use: simulation | real | famous")
                continue
            filtered_questions = [q for q in filtered_questions if getattr(q, 'source', None) == source_filter]
        elif choice == '3':
            status_filter = input("Enter status (new, reviewing, mastered): ").strip().lower()
            filtered_questions = [q for q in filtered_questions if q.user_status.lower() == status_filter]
        elif choice == '4':
            break
        elif choice != '1':
            print("Invalid filter choice.")
            continue
        
        if not filtered_questions:
            print("No questions found with selected filters.")
            continue

        print(f"\nFound {len(filtered_questions)} questions:")
        for i, q in enumerate(filtered_questions):
            print(f"{i+1}. ", end="")
            display_question_summary(q)
        
        detail_choice = input("\nEnter question number to view details, or 'b' to go back, 'm' for main menu: ").strip()
        if detail_choice.lower() == 'b':
            continue
        if detail_choice.lower() == 'm':
            break

        try:
            q_index = int(detail_choice) - 1
            if 0 <= q_index < len(filtered_questions):
                selected_q = filtered_questions[q_index]
                display_question_details(selected_q)
                
                while True:
                    status_action = input("\nChange status (n: new, r: reviewing, m: mastered, b: back to list, q: main menu): ").strip().lower()
                    if status_action == 'n':
                        update_question_status(selected_q.id, "new")
                        print(f"Question {selected_q.id} marked as 'new'.")
                        break
                    elif status_action == 'r':
                        update_question_status(selected_q.id, "reviewing")
                        print(f"Question {selected_q.id} marked as 'reviewing'.")
                        break
                    elif status_action == 'm':
                        update_question_status(selected_q.id, "mastered")
                        print(f"Question {selected_q.id} marked as 'mastered'.")
                        break
                    elif status_action == 'b':
                        break
                    elif status_action == 'q':
                        return # Go back to main menu
                    else:
                        print("Invalid status action.")
            else:
                print("Invalid question number.")
        except ValueError:
            print("Invalid input.")

def main_menu():
    """Main interactive loop."""
    print("Loading local data...")
    current_data = load_data()
    print(f"Loaded {len(current_data.simulation) + len(current_data.real) + len(current_data.famous)} questions.")

    while True:
        print("\n--- ErrorTK CLI ---")
        print("0. Settings (参数设置)")
        print("1. Sync data (Fetch from API)")
        print("2. List and review questions")
        print("3. Exit")
        
        choice = input("Enter your choice: ").strip()

        if choice == '0':
            settings_menu()
        elif choice == '1':
            if sync_data():
                current_data = load_data() # Reload data after sync
        elif choice == '2':
            list_questions(current_data)
            current_data = load_data() # Reload data after status changes
        elif choice == '3':
            print("Exiting ErrorTK CLI. Goodbye!")
            break
        else:
            print("Invalid choice. Please try again.")

if __name__ == "__main__":
    main_menu()
